use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};

use crate::state::Escrow;

pub fn process_take_instruction(accounts: &[AccountView], _data: &[u8]) -> ProgramResult {
    let [taker, maker, mint_a, mint_b, escrow_account, taker_ata_b, maker_ata_b, taker_ata_a, escrow_ata, _token_program, _remaining @ ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !taker.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !escrow_account.owned_by(&crate::ID) {
        return Err(ProgramError::IllegalOwner);
    }

    let (maker_from_state, amount_to_receive, amount_to_give, bump) = {
        let escrow_state = Escrow::from_account_info(escrow_account)?;
        (
            escrow_state.maker(),
            escrow_state.amount_to_receive(),
            escrow_state.amount_to_give(),
            escrow_state.bump,
        )
    };

    if maker_from_state != *maker.address() {
        return Err(ProgramError::InvalidAccountData);
    }

    let expected_escrow = pinocchio_pubkey::derive_address(
        &[b"escrow", maker.address().as_ref(), &[bump]],
        None,
        &crate::ID.to_bytes(),
    );
    if expected_escrow != *escrow_account.address().as_array() {
        return Err(ProgramError::InvalidSeeds);
    }

    let taker_ata_b_state = pinocchio_token::state::TokenAccount::from_account_view(taker_ata_b)?;
    if taker_ata_b_state.owner() != taker.address() || taker_ata_b_state.mint() != mint_b.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let maker_ata_b_state = pinocchio_token::state::TokenAccount::from_account_view(maker_ata_b)?;
    if maker_ata_b_state.owner() != maker.address() || maker_ata_b_state.mint() != mint_b.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let taker_ata_a_state = pinocchio_token::state::TokenAccount::from_account_view(taker_ata_a)?;
    if taker_ata_a_state.owner() != taker.address() || taker_ata_a_state.mint() != mint_a.address()
    {
        return Err(ProgramError::InvalidAccountData);
    }

    let escrow_ata_state = pinocchio_token::state::TokenAccount::from_account_view(escrow_ata)?;
    if escrow_ata_state.owner() != escrow_account.address()
        || escrow_ata_state.mint() != mint_a.address()
        || escrow_ata_state.amount() < amount_to_give
    {
        return Err(ProgramError::InvalidAccountData);
    }

    pinocchio_token::instructions::Transfer {
        from: taker_ata_b,
        to: maker_ata_b,
        authority: taker,
        amount: amount_to_receive,
    }
    .invoke()?;

    let bump_seed = [bump.to_le()];
    let signer_seed = [
        Seed::from(b"escrow"),
        Seed::from(maker.address().as_array()),
        Seed::from(&bump_seed),
    ];
    let signer = Signer::from(&signer_seed);

    pinocchio_token::instructions::Transfer {
        from: escrow_ata,
        to: taker_ata_a,
        authority: escrow_account,
        amount: amount_to_give,
    }
    .invoke_signed(&[signer.clone()])?;

    pinocchio_token::instructions::CloseAccount {
        account: escrow_ata,
        destination: maker,
        authority: escrow_account,
    }
    .invoke_signed(&[signer])?;

    let maker_lamports = maker.lamports();
    maker.set_lamports(maker_lamports.saturating_add(escrow_account.lamports()));
    escrow_account.close()?;

    Ok(())
}
