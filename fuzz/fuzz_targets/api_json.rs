//! Every JSON body the server accepts, deserialised from hostile input
//! (including the base64 fields). Nothing may panic.
#![no_main]

use libfuzzer_sys::fuzz_target;
use thencloud_crypto::api::*;

fn parse<T: serde::de::DeserializeOwned>(data: &[u8]) {
    let _ = serde_json::from_slice::<T>(data);
}

fuzz_target!(|data: &[u8]| {
    parse::<PreloginRequest>(data);
    parse::<RegisterRequest>(data);
    parse::<LoginRequest>(data);
    parse::<SetPqKeyRequest>(data);
    parse::<PasskeyRequest>(data);
    parse::<SecondFactorRequest>(data);
    parse::<EnableTotpRequest>(data);
    parse::<RegisterPasskeyRequest>(data);
    parse::<PasskeyLoginRequest>(data);
    parse::<CreateAppPasswordRequest>(data);
    parse::<AppLoginRequest>(data);
    parse::<ChangePasswordRequest>(data);
    parse::<CreateFolderRequest>(data);
    parse::<UpdateNodeRequest>(data);
    parse::<RestoreVersionRequest>(data);
    parse::<RestoreTrashRequest>(data);
    parse::<CreateUploadRequest>(data);
    parse::<CreateShareRequest>(data);
    parse::<UpdateShareRequest>(data);
    parse::<CreateLinkRequest>(data);
    parse::<UnlockLinkRequest>(data);
    parse::<AdoptDropRequest>(data);
    parse::<FinishUploadRequest>(data);
    parse::<UpdateUserRequest>(data);
    parse::<UpdateSettingsRequest>(data);
    parse::<CreateInviteRequest>(data);
    parse::<SetRecoveryRequest>(data);
    parse::<RemoveRecoveryRequest>(data);
    parse::<DeleteAccountRequest>(data);
    parse::<RecoveryUnlockRequest>(data);
    parse::<RecoveryResetRequest>(data);
    parse::<PutPrivateData>(data);
    parse::<VideoLinkRequest>(data);
});
