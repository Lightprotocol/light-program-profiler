use light_program_profiler::mollusk::{register_profiling_syscalls, take_profiling_results};
use mollusk_svm::{result::Check, Mollusk};
use solana_pubkey::Pubkey;

#[test]
fn test_profiled_program() {
    std::env::set_var("SBF_OUT_DIR", "../target/deploy");

    let program_id = Pubkey::new_unique();
    let mut mollusk = Mollusk::default();
    register_profiling_syscalls(&mut mollusk);
    mollusk.add_program(
        &program_id,
        "test_program",
        &mollusk_svm::program::loader_keys::LOADER_V3,
    );

    let instruction = solana_instruction::Instruction::new_with_bytes(program_id, &[5], vec![]);

    mollusk.process_and_validate_instruction(&instruction, &[], &[Check::success()]);

    let profiling = take_profiling_results();
    assert_eq!(
        profiling.len(),
        1,
        "Expected 1 profiling entry, got {}",
        profiling.len()
    );
    let (func_name, cu_consumed, file_location) = &profiling[0];
    assert_eq!(func_name, "profiled_function");
    assert!(*cu_consumed > 0, "CU consumed should be non-zero");
    assert!(
        file_location.contains("test-program/src/lib.rs"),
        "Unexpected file location: {}",
        file_location
    );
    println!(
        "SUCCESS: {} consumed {} CU at {}",
        func_name, cu_consumed, file_location
    );
}
