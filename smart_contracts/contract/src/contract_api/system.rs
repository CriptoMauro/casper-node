//! Functions for interacting with the system contracts.

use alloc::vec::Vec;
use core::mem::MaybeUninit;

use casper_types::{
    account::AccountHash,
    api_error, bytesrepr,
    system::{
        auction::{self, EraId, EraInfo},
        SystemContractType,
    },
    ApiError, ContractHash, HashAddr, TransferResult, TransferredTo, URef, U512,
    UREF_SERIALIZED_LENGTH,
};

use crate::{
    contract_api::{self, account, runtime},
    ext_ffi,
    unwrap_or_revert::UnwrapOrRevert,
};

fn get_system_contract(system_contract: SystemContractType) -> ContractHash {
    let system_contract_index = system_contract.into();
    let contract_hash: ContractHash = {
        let result = {
            let mut hash_data_raw: HashAddr = ContractHash::default().value();
            let value = unsafe {
                ext_ffi::casper_get_system_contract(
                    system_contract_index,
                    hash_data_raw.as_mut_ptr(),
                    hash_data_raw.len(),
                )
            };
            api_error::result_from(value).map(|_| hash_data_raw)
        };
        // Revert for any possible error that happened on host side
        let contract_hash_bytes = result.unwrap_or_else(|e| runtime::revert(e));
        // Deserializes a valid URef passed from the host side
        bytesrepr::deserialize(contract_hash_bytes.to_vec()).unwrap_or_revert()
    };
    contract_hash
}

/// Returns a read-only pointer to the Mint contract.
///
/// Any failure will trigger [`revert`](runtime::revert) with an appropriate [`ApiError`].
pub fn get_mint() -> ContractHash {
    get_system_contract(SystemContractType::Mint)
}

/// Returns a read-only pointer to the Proof of Stake contract.
///
/// Any failure will trigger [`revert`](runtime::revert) with an appropriate [`ApiError`].
pub fn get_proof_of_stake() -> ContractHash {
    get_system_contract(SystemContractType::ProofOfStake)
}

/// Returns a read-only pointer to the Standard Payment contract.
///
/// Any failure will trigger [`revert`](runtime::revert) with an appropriate [`ApiError`].
pub fn get_standard_payment() -> ContractHash {
    get_system_contract(SystemContractType::StandardPayment)
}

/// Returns a read-only pointer to the Auction contract.
///
/// Any failure will trigger [`revert`](runtime::revert) with appropriate [`ApiError`].
pub fn get_auction() -> ContractHash {
    get_system_contract(SystemContractType::Auction)
}

/// Creates a new empty purse and returns its [`URef`].
#[doc(hidden)]
pub fn create_purse() -> URef {
    let purse_non_null_ptr = contract_api::alloc_bytes(UREF_SERIALIZED_LENGTH);
    unsafe {
        let ret = ext_ffi::casper_create_purse(purse_non_null_ptr.as_ptr(), UREF_SERIALIZED_LENGTH);
        if ret == 0 {
            let bytes = Vec::from_raw_parts(
                purse_non_null_ptr.as_ptr(),
                UREF_SERIALIZED_LENGTH,
                UREF_SERIALIZED_LENGTH,
            );
            bytesrepr::deserialize(bytes).unwrap_or_revert()
        } else {
            runtime::revert(ApiError::PurseNotCreated)
        }
    }
}

/// Returns the balance in motes of the given purse.
#[doc(hidden)]
pub fn get_purse_balance(purse: URef) -> Option<U512> {
    let (purse_ptr, purse_size, _bytes) = contract_api::to_ptr(purse);

    let value_size = {
        let mut output_size = MaybeUninit::uninit();
        let ret =
            unsafe { ext_ffi::casper_get_balance(purse_ptr, purse_size, output_size.as_mut_ptr()) };
        match api_error::result_from(ret) {
            Ok(_) => unsafe { output_size.assume_init() },
            Err(ApiError::InvalidPurse) => return None,
            Err(error) => runtime::revert(error),
        }
    };
    let value_bytes = runtime::read_host_buffer(value_size).unwrap_or_revert();
    let value: U512 = bytesrepr::deserialize(value_bytes).unwrap_or_revert();
    Some(value)
}

/// Returns the balance in motes of a purse.
pub fn get_balance() -> Option<U512> {
    get_purse_balance(account::get_main_purse())
}

/// Transfers `amount` of motes from the default purse of the account to `target`
/// account.  If `target` does not exist it will be created.
pub fn transfer_to_account(target: AccountHash, amount: U512, id: Option<u64>) -> TransferResult {
    let (target_ptr, target_size, _bytes1) = contract_api::to_ptr(target);
    let (amount_ptr, amount_size, _bytes2) = contract_api::to_ptr(amount);
    let (id_ptr, id_size, _bytes3) = contract_api::to_ptr(id);
    let mut maybe_result_value = MaybeUninit::uninit();

    let return_code = unsafe {
        ext_ffi::casper_transfer_to_account(
            target_ptr,
            target_size,
            amount_ptr,
            amount_size,
            id_ptr,
            id_size,
            maybe_result_value.as_mut_ptr(),
        )
    };

    // Propagate error (if any)
    api_error::result_from(return_code)?;

    // Return appropriate result if transfer was successful
    let transferred_to_value = unsafe { maybe_result_value.assume_init() };
    TransferredTo::result_from(transferred_to_value)
}

/// Transfers `amount` of motes from `source` purse to `target` account.  If `target` does not exist
/// it will be created.
#[doc(hidden)]
pub fn transfer_from_purse_to_account(
    source: URef,
    target: AccountHash,
    amount: U512,
    id: Option<u64>,
) -> TransferResult {
    let (source_ptr, source_size, _bytes1) = contract_api::to_ptr(source);
    let (target_ptr, target_size, _bytes2) = contract_api::to_ptr(target);
    let (amount_ptr, amount_size, _bytes3) = contract_api::to_ptr(amount);
    let (id_ptr, id_size, _bytes4) = contract_api::to_ptr(id);

    let mut maybe_result_value = MaybeUninit::uninit();
    let return_code = unsafe {
        ext_ffi::casper_transfer_from_purse_to_account(
            source_ptr,
            source_size,
            target_ptr,
            target_size,
            amount_ptr,
            amount_size,
            id_ptr,
            id_size,
            maybe_result_value.as_mut_ptr(),
        )
    };

    // Propagate error (if any)
    api_error::result_from(return_code)?;

    // Return appropriate result if transfer was successful
    let transferred_to_value = unsafe { maybe_result_value.assume_init() };
    TransferredTo::result_from(transferred_to_value)
}

/// Transfers `amount` of motes from `source` purse to `target` purse.  If `target` does not exist
/// the transfer fails.
#[doc(hidden)]
pub fn transfer_from_purse_to_purse(
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
) -> Result<(), ApiError> {
    let (source_ptr, source_size, _bytes1) = contract_api::to_ptr(source);
    let (target_ptr, target_size, _bytes2) = contract_api::to_ptr(target);
    let (amount_ptr, amount_size, _bytes3) = contract_api::to_ptr(amount);
    let (id_ptr, id_size, _bytes4) = contract_api::to_ptr(id);
    let result = unsafe {
        ext_ffi::casper_transfer_from_purse_to_purse(
            source_ptr,
            source_size,
            target_ptr,
            target_size,
            amount_ptr,
            amount_size,
            id_ptr,
            id_size,
        )
    };
    api_error::result_from(result)
}

/// Records a transfer.  Can only be called from within the mint contract.
/// Needed to support system contract-based execution.
#[doc(hidden)]
pub fn record_transfer(
    maybe_to: Option<AccountHash>,
    source: URef,
    target: URef,
    amount: U512,
    id: Option<u64>,
) -> Result<(), ApiError> {
    let (maybe_to_ptr, maybe_to_size, _bytes1) = contract_api::to_ptr(maybe_to);
    let (source_ptr, source_size, _bytes2) = contract_api::to_ptr(source);
    let (target_ptr, target_size, _bytes3) = contract_api::to_ptr(target);
    let (amount_ptr, amount_size, _bytes4) = contract_api::to_ptr(amount);
    let (id_ptr, id_size, _bytes5) = contract_api::to_ptr(id);
    let result = unsafe {
        ext_ffi::casper_record_transfer(
            maybe_to_ptr,
            maybe_to_size,
            source_ptr,
            source_size,
            target_ptr,
            target_size,
            amount_ptr,
            amount_size,
            id_ptr,
            id_size,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(ApiError::Transfer)
    }
}

/// Records era info.  Can only be called from within the auction contract.
/// Needed to support system contract-based execution.
#[doc(hidden)]
pub fn record_era_info(era_id: EraId, era_info: EraInfo) -> Result<(), ApiError> {
    let (era_id_ptr, era_id_size, _bytes1) = contract_api::to_ptr(era_id);
    let (era_info_ptr, era_info_size, _bytes2) = contract_api::to_ptr(era_info);
    let result = unsafe {
        ext_ffi::casper_record_era_info(era_id_ptr, era_id_size, era_info_ptr, era_info_size)
    };
    if result == 0 {
        Ok(())
    } else {
        Err(auction::Error::RecordEraInfo.into())
    }
}
