use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};
use pinocchio_pubkey::derive_address;

use crate::state::Escrow;

pub fn process_take_instruction(accounts: &[AccountView], _data: &[u8]) -> ProgramResult {
    let [taker, maker, mint_a, mint_b, escrow_account, taker_ata_b, maker_ata_b, taker_ata_a, escrow_ata, _token_program, _remaining @ ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    let taker_ata_b_state = pinocchio_token::state::TokenAccount::from_account_view(&taker_ata_b)?;
    if taker_ata_b_state.owner() != taker.address() {
        return Err(ProgramError::IllegalOwner);
    }
    if taker_ata_b_state.mint() != mint_b.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let escrow_state = Escrow::from_account_info(escrow_account)?;
    assert_eq!(escrow_state.maker(), *maker.address());
    assert_eq!(escrow_state.mint_a(), *mint_a.address());
    assert_eq!(escrow_state.mint_b(), *mint_b.address());

    let bump = escrow_state.bump;
    let amount_to_receive = escrow_state.amount_to_receive();
    let amount_to_give = escrow_state.amount_to_give();

    let seed = [b"escrow".as_ref(), maker.address().as_ref(), &[bump]];
    let escrow_account_pda = derive_address(&seed, None, &crate::ID.to_bytes());
    assert_eq!(escrow_account_pda, *escrow_account.address().as_array());

    pinocchio_token::instructions::Transfer {
        from: taker_ata_b,
        to: maker_ata_b,
        authority: taker,
        amount: amount_to_receive,
    }
    .invoke()?;

    let bump = [bump.to_le()];
    let seed = [
        Seed::from(b"escrow"),
        Seed::from(maker.address().as_array()),
        Seed::from(&bump),
    ];
    let seeds = Signer::from(&seed);

    pinocchio_token::instructions::Transfer {
        from: escrow_ata,
        to: taker_ata_a,
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
