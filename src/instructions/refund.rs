use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};
use pinocchio_pubkey::derive_address;

use crate::state::Escrow;

pub fn process_refund_instruction(accounts: &[AccountView], _data: &[u8]) -> ProgramResult {
    let [maker, mint_a, mint_b, escrow_account, maker_ata_a, escrow_ata, _token_program, _remaining @ ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let maker_ata_a_state = pinocchio_token::state::TokenAccount::from_account_view(&maker_ata_a)?;
    if maker_ata_a_state.owner() != maker.address() {
        return Err(ProgramError::IllegalOwner);
    }
    if maker_ata_a_state.mint() != mint_a.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let escrow_state = Escrow::from_account_info(escrow_account)?;
    assert_eq!(escrow_state.maker(), *maker.address());
    assert_eq!(escrow_state.mint_a(), *mint_a.address());
    assert_eq!(escrow_state.mint_b(), *mint_b.address());

    let bump = escrow_state.bump;
    let amount_to_give = escrow_state.amount_to_give();

    let seed = [b"escrow".as_ref(), maker.address().as_ref(), &[bump]];
    let escrow_account_pda = derive_address(&seed, None, &crate::ID.to_bytes());
    assert_eq!(escrow_account_pda, *escrow_account.address().as_array());

    let bump = [bump.to_le()];
    let seed = [
        Seed::from(b"escrow"),
        Seed::from(maker.address().as_array()),
        Seed::from(&bump),
    ];
    let seeds = Signer::from(&seed);

    pinocchio_token::instructions::Transfer {
        from: escrow_ata,
        to: maker_ata_a,
        authority: escrow_account,
        amount: amount_to_give,
    }
    .invoke_signed(&[seeds.clone()])?;

    pinocchio_token::instructions::CloseAccount {
        account: escrow_ata,
        destination: maker,
        authority: escrow_account,
    }
    .invoke_signed(&[seeds])?;

    maker.set_lamports(maker.lamports().saturating_add(escrow_account.lamports()));
    escrow_account.close()?;

    Ok(())
}
