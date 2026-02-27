use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    AccountView, ProgramResult,
};

use crate::state::Escrow;

pub fn process_refund_instruction(accounts: &[AccountView], _data: &[u8]) -> ProgramResult {
    let [maker, mint_a, mint_b, escrow_account, maker_ata_a, escrow_ata, _token_program, _remaining @ ..] =
        accounts
    else {
        return Err(ProgramError::NotEnoughAccountKeys);
    };

    if !maker.is_signer() {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !escrow_account.owned_by(&crate::ID) {
        return Err(ProgramError::IllegalOwner);
    }

    let (maker_from_state, mint_a_from_state, mint_b_from_state, amount_to_give, bump) = {
        let escrow_state = Escrow::from_account_info(escrow_account)?;
        (
            escrow_state.maker(),
            escrow_state.mint_a(),
            escrow_state.mint_b(),
            escrow_state.amount_to_give(),
            escrow_state.bump,
        )
    };

    if maker_from_state != *maker.address()
        || mint_a_from_state != *mint_a.address()
        || mint_b_from_state != *mint_b.address()
    {
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

    let maker_ata_a_state = pinocchio_token::state::TokenAccount::from_account_view(maker_ata_a)?;
    if maker_ata_a_state.owner() != maker.address() || maker_ata_a_state.mint() != mint_a.address()
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

    let bump_seed = [bump.to_le()];
    let signer_seed = [
        Seed::from(b"escrow"),
        Seed::from(maker.address().as_array()),
        Seed::from(&bump_seed),
    ];
    let signer = Signer::from(&signer_seed);

    pinocchio_token::instructions::Transfer {
        from: escrow_ata,
        to: maker_ata_a,
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
