use thiserror::Error;

use crate::{
    components::{
        contract_runtime, contract_runtime::BlockExecutionError, network, small_network, storage,
    },
    types::{Block, BlockHeader},
    utils::ListeningError,
};
use casper_execution_engine::core::engine_state;
use casper_types::{bytesrepr, EraId};

/// Error type returned by the validator reactor.
#[derive(Debug, Error)]
pub enum Error {
    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),

    /// `Network` component error.
    #[error("network error: {0}")]
    Network(#[from] network::Error),

    /// `SmallNetwork` component error.
    #[error("small network error: {0}")]
    SmallNetwork(#[from] small_network::Error),

    /// An error starting one of the HTTP servers.
    #[error("http server listening error: {0}")]
    ListeningError(#[from] ListeningError),

    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),

    /// `Consensus` component error.
    #[error("consensus error: {0}")]
    Consensus(#[from] anyhow::Error),

    /// `ContractRuntime` component error.
    #[error("contract runtime config error: {0}")]
    ContractRuntime(#[from] contract_runtime::ConfigError),

    /// Failed to serialize data.
    #[error("serialization: {0}")]
    Serialization(#[source] bincode::ErrorKind),

    /// Engine state error.
    #[error(transparent)]
    EngineState(#[from] engine_state::Error),

    /// Block execution error.
    #[error("block execution error: {0}")]
    BlockExecutionError(#[from] BlockExecutionError),

    /// [`bytesrepr`] error.
    #[error("bytesrepr error: {0}")]
    BytesReprError(#[from] bytesrepr::Error),

    /// Cannot run genesis on pre-existing blockchain.
    #[error("Cannot run genesis on pre-existing blockchain. First block: {first_block_header:?}")]
    CannotRunGenesisOnPreExistingBlockchain {
        /// The first block header.  Should have height 1.
        first_block_header: Box<BlockHeader>,
    },

    /// No such switch block for upgrade era.
    #[error("No such switch block for upgrade era: {upgrade_era_id}")]
    NoSuchSwitchBlockHeaderForUpgradeEra {
        /// The upgrade era id.
        upgrade_era_id: EraId,
    },

    /// Non-emergency upgrade will clobber existing blockchain.
    #[error(
        "Non-emergency upgrade will clobber existing blockchain. \
         Preexisting block header: {preexisting_block_header}"
    )]
    NonEmergencyUpgradeWillClobberExistingBlockChain {
        /// A preexisting block header.
        preexisting_block_header: Box<BlockHeader>,
    },

    /// Failed to create a switch block immediately after genesis or upgrade.
    #[error(
        "Failed to create a switch block immediately after genesis or upgrade. \
         New bad block we made: {new_bad_block}"
    )]
    FailedToCreateSwitchBlockAfterGenesisOrUpgrade {
        /// A new block we made which should be a switch block but is not.
        new_bad_block: Box<Block>,
    },
}
