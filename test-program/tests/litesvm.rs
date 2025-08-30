use litesvm::LiteSVM;
use solana_instruction::Instruction;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use solana_transaction::Transaction;

#[test]
fn test_profiled_program() {
    let mut svm = LiteSVM::new();

    // Deploy the test program
    let program_id = Pubkey::new_unique();
    let program_bytes = include_bytes!("../../target/deploy/test_program.so");
    svm.add_program(program_id, program_bytes);

    // Create payer account
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 1_000_000_000).unwrap();

    // Create instruction with some test data
    let instruction = Instruction::new_with_bytes(
        program_id,
        &[5], // This will cause the loop to run 5 times
        vec![],
    );

    // Create and send transaction
    let blockhash = svm.latest_blockhash();
    let transaction = Transaction::new_signed_with_payer(
        &[instruction],
        Some(&payer.pubkey()),
        &[&payer],
        blockhash,
    );

    let result = svm.send_transaction(transaction);
    assert!(result.is_ok(), "Transaction failed: {:?}", result);

    // Check logs for profiling output
    let meta = result.unwrap();
    let expected_res = "Program log: #  1    profiled_function\n        test-program/src/lib.rs:19                          \nCU                                                  consumed  1909 (net  1909) of 199887 CU";
    assert!(
        meta.logs.iter().any(|x| x.contains(expected_res)),
        "logs: {:?}",
        meta.logs
    );
}
