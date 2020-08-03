//! Command-line option parsing.
//!
//! Most configuration is done via config files (see [`config`](../config/index.html) for details).

use std::{
    env, io,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{self, bail, Context};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use regex::Regex;
use structopt::StructOpt;
use toml::{value::Table, Value};
use tracing::{info, trace};

use crate::config;
use casperlabs_node::{
    logging,
    reactor::{initializer, validator, Runner},
    tls,
};

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
/// CasperLabs blockchain node.
pub enum Cli {
    /// Generate a self-signed node certificate.
    GenerateCert {
        /// Output path base of the certificate. The certificate will be stored as
        /// `output.crt.pem`, while the key will be stored as `output.key.pem`.
        output: PathBuf,
    },
    /// Generate a configuration file from defaults and dump it to stdout.
    GenerateConfig {},

    /// Run the validator node.
    ///
    /// Loads the configuration values from the given configuration file or uses defaults if not
    /// given, then runs the reactor.
    Validator {
        #[structopt(short, long, env)]
        /// Path to configuration file.
        config: Option<PathBuf>,

        #[structopt(short = "C", long, env = "NODE_CONFIG", use_delimiter(true))]
        /// Overrides and extensions for configuration file entries in the form
        /// <SECTION>.<KEY>=<VALUE>.  For example, '-C=node.chainspec_config_path=chainspec.toml'
        config_ext: Vec<ConfigExt>,
    },
}

#[derive(Debug)]
/// Command line extension to be applied to TOML-based config file values.
pub struct ConfigExt {
    section: String,
    key: String,
    value: String,
}

impl ConfigExt {
    /// Updates TOML table with updated or extended key value pairs.
    fn update_toml_table(&self, toml_value: &mut Value) -> Option<()> {
        let table = toml_value.as_table_mut()?;
        if !table.contains_key(&self.section) {
            table.insert(self.section.clone(), Value::Table(Table::new()));
        }
        let val = parse_toml_value(&self.value);
        table[&self.section]
            .as_table_mut()?
            .insert(self.key.clone(), val);
        Some(())
    }
}

impl FromStr for ConfigExt {
    type Err = anyhow::Error;

    /// Attempts to create a ConfigExt from a str patterned as `section.key=value`
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^([^.]+)\.([^=]+)=(.+)$").unwrap();
        let captures = re
            .captures(input)
            .context("could not parse config_ext (see README.md)")?;
        Ok(ConfigExt {
            section: captures
                .get(1)
                .context("failed to find section")?
                .as_str()
                .to_owned(),
            key: captures
                .get(2)
                .context("failed to find key")?
                .as_str()
                .to_owned(),
            value: captures
                .get(3)
                .context("failed to find value")?
                .as_str()
                .to_owned(),
        })
    }
}

/// Convenience function to parse values passed via command line into appropriate `toml::Value`
/// representations.
fn parse_toml_value(raw: &str) -> Value {
    if let Ok(value) = i64::from_str(raw) {
        return Value::Integer(value);
    }
    if let Ok(value) = bool::from_str(raw) {
        return Value::Boolean(value);
    }
    Value::String(raw.to_string())
}

/// Normalizes any config key ending with `path` and a value that appears to be a relative path,
/// relative to the config file path.
fn normalize_paths(maybe_config_dir: Option<PathBuf>, config: &mut Value) {
    let table = if let Value::Table(table) = config {
        table
    } else {
        return;
    };

    for (_section, inner) in table {
        let table = if let Value::Table(table) = inner {
            table
        } else {
            continue;
        };

        for (k, v) in table {
            // Skip any key that does not appear to be a file path.
            if !k.ends_with("path") {
                continue;
            }

            if let Value::String(path_str) = v {
                // Replace env vars in the provided path.
                for (env_var_name, env_var_value) in env::vars() {
                    *path_str = path_str.replace(&format!("${}", env_var_name), &env_var_value);
                }

                let path = Path::new(path_str);
                if path.is_relative() {
                    if let Some(root_dir) = maybe_config_dir.as_ref() {
                        *path_str = root_dir.join(path).display().to_string();
                    }
                }
            }
        }
    }
}

impl Cli {
    /// Executes selected CLI command.
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Cli::GenerateCert { output } => {
                if output.file_name().is_none() {
                    bail!("not a valid output path");
                }

                let mut cert_path = output.clone();
                cert_path.set_extension("crt.pem");

                let mut key_path = output;
                key_path.set_extension("key.pem");

                let (cert, key) = tls::generate_node_cert()?;

                tls::save_cert(&cert, cert_path)?;
                tls::save_private_key(&key, key_path)?;
            }
            Cli::GenerateConfig {} => {
                let cfg_str = config::to_string(&validator::Config::default())?;
                io::stdout().write_all(cfg_str.as_bytes())?;
            }
            Cli::Validator { config, config_ext } => {
                // The app supports running without a config file, using default values.
                let maybe_config: Option<validator::Config> =
                    config.as_ref().map(config::load_from_file).transpose()?;

                // Get the TOML table version of the config indicated from CLI args, or from a new
                // defaulted config instance if one is not provided.
                let mut config_table: Value =
                    toml::from_str(&toml::to_string(&maybe_config.unwrap_or_default())?)?;

                // If any command line overrides to the config values are passed, apply them.
                for item in config_ext {
                    item.update_toml_table(&mut config_table);
                }

                // If a config file path to a TOML file was provided, normalize relative paths in
                // the config to the config file's path.
                // If a config file path was not passed via CLI and a default config instance is
                // being used instead, do not normalize paths.
                let maybe_root_path =
                    config.map(|p| p.canonicalize().unwrap().parent().unwrap().to_path_buf());

                normalize_paths(maybe_root_path, &mut config_table);

                // Create validator config, including any overridden or normalized values.
                let validator_config: validator::Config = config_table.try_into()?;
                logging::init_with_config(&validator_config.logging)?;
                trace!("{}", config::to_string(&validator_config)?);

                // We use a `ChaCha20Rng` for the production node. For one, we want to completely
                // eliminate any chance of runtime failures, regardless of how small (these
                // exist with `OsRng`). Additionally, we want to limit the number of syscalls for
                // performance reasons.
                let mut rng = ChaCha20Rng::from_entropy();

                let mut runner =
                    Runner::<initializer::Reactor>::new(validator_config, &mut rng).await?;
                runner.run(&mut rng).await;

                info!("finished initialization");

                let initializer = runner.into_inner();
                if !initializer.stopped_successfully() {
                    bail!("failed to initialize successfully");
                }

                let mut runner = Runner::<validator::Reactor>::new(initializer, &mut rng).await?;
                runner.run(&mut rng).await;
            }
        }

        Ok(())
    }
}
