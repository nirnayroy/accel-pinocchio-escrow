#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use litesvm::LiteSVM;
    use litesvm_token::{
        get_spl_account,
        spl_token::{self, state::Account as SplTokenAccount},
        CreateAssociatedTokenAccount, CreateMint, MintTo,
    };
    use solana_instruction::{AccountMeta, Instruction};
    use solana_keypair::Keypair;
    use solana_message::Message;
    use solana_native_token::LAMPORTS_PER_SOL;
    use solana_pubkey::Pubkey;
    use solana_signer::Signer;
    use solana_transaction::Transaction;

    const PROGRAM_ID: &str = "4ibrEMW5F6hKnkW4jVedswYv6H6VtwPN6ar6dvXDN1nT";
    const TOKEN_PROGRAM_ID: Pubkey = spl_token::ID;
    const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

    fn program_id() -> Pubkey {
        Pubkey::from(crate::ID)
    }

    fn program_so_path() -> Option<PathBuf> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let candidates = [
            manifest_dir.join("target/deploy/escrow.so"),
            manifest_dir.join("target/sbf-solana-solana/release/escrow.so"),
            manifest_dir.join("target/sbf-solana-solana/release/libescrow.so"),
        ];

        candidates.into_iter().find(|path| path.exists())
    }

    fn setup() -> Option<(LiteSVM, Keypair, Keypair)> {
        let Some(so_path) = program_so_path() else {
            eprintln!(
                "Skipping LiteSVM tests: program .so not found. Build with `cargo build-sbf`."
            );
            return None;
        };

        let mut svm = LiteSVM::new();
        let maker = Keypair::new();
        let taker = Keypair::new();

        svm.airdrop(&maker.pubkey(), 10 * LAMPORTS_PER_SOL)
            .expect("Maker airdrop failed");
        svm.airdrop(&taker.pubkey(), 10 * LAMPORTS_PER_SOL)
            .expect("Taker airdrop failed");

        let program_data = std::fs::read(so_path).expect("Failed to read program SO file");
        svm.add_program(program_id(), &program_data)
            .expect("Failed to add program");

        Some((svm, maker, taker))
    }

    fn escrow_pda(maker: &Pubkey) -> (Pubkey, u8) {
        Pubkey::find_program_address(&[b"escrow", maker.as_ref()], &program_id())
    }

    fn token_balance(svm: &LiteSVM, account: &Pubkey) -> u64 {
        get_spl_account::<SplTokenAccount>(svm, account)
            .expect("token account should exist")
            .amount
    }

    fn send_ix(svm: &mut LiteSVM, payer: &Keypair, ix: Instruction, extra_signers: &[&Keypair]) {
        let message = Message::new(&[ix], Some(&payer.pubkey()));
        let blockhash = svm.latest_blockhash();

        let mut signers = vec![payer];
        signers.extend_from_slice(extra_signers);

        let tx = Transaction::new(&signers, message, blockhash);
        svm.send_transaction(tx)
            .expect("transaction should succeed");
    }

    fn make_ix(
        maker: &Pubkey,
        mint_a: Pubkey,
        mint_b: Pubkey,
        escrow: Pubkey,
        maker_ata_a: Pubkey,
        vault: Pubkey,
        bump: u8,
        amount_to_receive: u64,
        amount_to_give: u64,
    ) -> Instruction {
        let make_data = [
            vec![0u8],
            bump.to_le_bytes().to_vec(),
            amount_to_receive.to_le_bytes().to_vec(),
            amount_to_give.to_le_bytes().to_vec(),
        ]
        .concat();

        Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(*maker, true),
                AccountMeta::new(mint_a, false),
                AccountMeta::new(mint_b, false),
                AccountMeta::new(escrow, false),
                AccountMeta::new(maker_ata_a, false),
                AccountMeta::new(vault, false),
                AccountMeta::new(solana_sdk_ids::system_program::ID, false),
                AccountMeta::new(TOKEN_PROGRAM_ID, false),
                AccountMeta::new(ASSOCIATED_TOKEN_PROGRAM_ID.parse().unwrap(), false),
            ],
            data: make_data,
        }
    }

    fn take_ix(
        taker: &Pubkey,
        maker: &Pubkey,
        mint_a: Pubkey,
        mint_b: Pubkey,
        escrow: Pubkey,
        taker_ata_b: Pubkey,
        maker_ata_b: Pubkey,
        taker_ata_a: Pubkey,
        vault: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(*taker, true),
                AccountMeta::new_readonly(*maker, false),
                AccountMeta::new_readonly(mint_a, false),
                AccountMeta::new_readonly(mint_b, false),
                AccountMeta::new(escrow, false),
                AccountMeta::new(taker_ata_b, false),
                AccountMeta::new(maker_ata_b, false),
                AccountMeta::new(taker_ata_a, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: vec![1u8],
        }
    }

    fn refund_ix(
        maker: &Pubkey,
        mint_a: Pubkey,
        mint_b: Pubkey,
        escrow: Pubkey,
        maker_ata_a: Pubkey,
        vault: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: program_id(),
            accounts: vec![
                AccountMeta::new(*maker, true),
                AccountMeta::new_readonly(mint_a, false),
                AccountMeta::new_readonly(mint_b, false),
                AccountMeta::new(escrow, false),
                AccountMeta::new(maker_ata_a, false),
                AccountMeta::new(vault, false),
                AccountMeta::new_readonly(TOKEN_PROGRAM_ID, false),
            ],
            data: vec![2u8],
        }
    }

    #[test]
    fn test_make_instruction() {
        let Some((mut svm, maker, _taker)) = setup() else {
            return;
        };

        assert_eq!(program_id().to_string(), PROGRAM_ID);

        let mint_a = CreateMint::new(&mut svm, &maker)
            .decimals(6)
            .authority(&maker.pubkey())
            .send()
            .unwrap();
        let mint_b = CreateMint::new(&mut svm, &maker)
            .decimals(6)
            .authority(&maker.pubkey())
            .send()
            .unwrap();

        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a)
            .owner(&maker.pubkey())
            .send()
            .unwrap();

        let amount_to_receive: u64 = 100_000_000;
        let amount_to_give: u64 = 500_000_000;

        MintTo::new(&mut svm, &maker, &mint_a, &maker_ata_a, amount_to_give)
            .send()
            .unwrap();

        let (escrow, bump) = escrow_pda(&maker.pubkey());
        let vault = spl_associated_token_account::get_associated_token_address(&escrow, &mint_a);

        let make = make_ix(
            &maker.pubkey(),
            mint_a,
            mint_b,
            escrow,
            maker_ata_a,
            vault,
            bump,
            amount_to_receive,
            amount_to_give,
        );
        send_ix(&mut svm, &maker, make, &[]);

        assert_eq!(token_balance(&svm, &maker_ata_a), 0);
        assert_eq!(token_balance(&svm, &vault), amount_to_give);
    }

    #[test]
    fn test_take_instruction() {
        let Some((mut svm, maker, taker)) = setup() else {
            return;
        };

        assert_eq!(program_id().to_string(), PROGRAM_ID);

        let mint_a = CreateMint::new(&mut svm, &maker)
            .decimals(6)
            .authority(&maker.pubkey())
            .send()
            .unwrap();
        let mint_b = CreateMint::new(&mut svm, &maker)
            .decimals(6)
            .authority(&maker.pubkey())
            .send()
            .unwrap();

        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a)
            .owner(&maker.pubkey())
            .send()
            .unwrap();
        let maker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_b)
            .owner(&maker.pubkey())
            .send()
            .unwrap();

        let taker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &taker, &mint_a)
            .owner(&taker.pubkey())
            .send()
            .unwrap();
        let taker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &taker, &mint_b)
            .owner(&taker.pubkey())
            .send()
            .unwrap();

        let amount_to_receive: u64 = 100_000_000;
        let amount_to_give: u64 = 500_000_000;

        MintTo::new(&mut svm, &maker, &mint_a, &maker_ata_a, amount_to_give)
            .send()
            .unwrap();
        MintTo::new(&mut svm, &maker, &mint_b, &taker_ata_b, amount_to_receive)
            .send()
            .unwrap();

        let (escrow, bump) = escrow_pda(&maker.pubkey());
        let vault = spl_associated_token_account::get_associated_token_address(&escrow, &mint_a);

        let make = make_ix(
            &maker.pubkey(),
            mint_a,
            mint_b,
            escrow,
            maker_ata_a,
            vault,
            bump,
            amount_to_receive,
            amount_to_give,
        );
        send_ix(&mut svm, &maker, make, &[]);

        let take = take_ix(
            &taker.pubkey(),
            &maker.pubkey(),
            mint_a,
            mint_b,
            escrow,
            taker_ata_b,
            maker_ata_b,
            taker_ata_a,
            vault,
        );
        send_ix(&mut svm, &taker, take, &[]);

        assert_eq!(token_balance(&svm, &maker_ata_b), amount_to_receive);
        assert_eq!(token_balance(&svm, &taker_ata_a), amount_to_give);
        assert_eq!(token_balance(&svm, &taker_ata_b), 0);
        assert!(get_spl_account::<SplTokenAccount>(&svm, &vault).is_err());
    }

    #[test]
    fn test_refund_instruction() {
        let Some((mut svm, maker, _taker)) = setup() else {
            return;
        };

        let mint_a = CreateMint::new(&mut svm, &maker)
            .decimals(6)
            .authority(&maker.pubkey())
            .send()
            .unwrap();
        let mint_b = CreateMint::new(&mut svm, &maker)
            .decimals(6)
            .authority(&maker.pubkey())
            .send()
            .unwrap();

        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a)
            .owner(&maker.pubkey())
            .send()
            .unwrap();

        let amount_to_receive: u64 = 100_000_000;
        let amount_to_give: u64 = 500_000_000;

        MintTo::new(&mut svm, &maker, &mint_a, &maker_ata_a, amount_to_give)
            .send()
            .unwrap();

        let (escrow, bump) = escrow_pda(&maker.pubkey());
        let vault = spl_associated_token_account::get_associated_token_address(&escrow, &mint_a);

        let make = make_ix(
            &maker.pubkey(),
            mint_a,
            mint_b,
            escrow,
            maker_ata_a,
            vault,
            bump,
            amount_to_receive,
            amount_to_give,
        );
        send_ix(&mut svm, &maker, make, &[]);

        let refund = refund_ix(&maker.pubkey(), mint_a, mint_b, escrow, maker_ata_a, vault);
        send_ix(&mut svm, &maker, refund, &[]);

        assert_eq!(token_balance(&svm, &maker_ata_a), amount_to_give);
        assert!(get_spl_account::<SplTokenAccount>(&svm, &vault).is_err());
    }
}
