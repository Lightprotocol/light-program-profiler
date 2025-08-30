use light_program_profiler::profile;
use solana_program::{
    account_info::AccountInfo, entrypoint, msg, program_error::ProgramError, pubkey::Pubkey,
};

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(processor);

fn processor(
    _program_id: &Pubkey,
    _accounts: &[AccountInfo],

    ix_data: &[u8],
) -> Result<(), ProgramError> {
    profiled_function(ix_data);
    Ok(())
}

#[profile]
fn profiled_function(ix_data: &[u8]) {
    if !ix_data.is_empty() {
        for i in 0..ix_data[0] {
            msg!("i {}", i);
        }
    }
}
