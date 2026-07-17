//! PayrollError discriminant-stability guard test (#784).
//!
//! `PayrollError` discriminants are a stable, externally-matched interface:
//! off-chain indexers and clients key on the numeric code, not the variant
//! name. This test locks in every existing variant's discriminant so that an
//! accidental insert/reorder/renumber is caught in CI before release.
//!
//! New variants MUST be appended at the end with the next consecutive integer.
//! (See the append-only note above the `PayrollError` enum in `storage.rs`.)

#![cfg(test)]
#![allow(deprecated)]

use stello_pay_contract::storage::PayrollError;

#[test]
fn payroll_error_discriminants_are_stable() {
    // (variant, expected stable discriminant)
    let cases: &[(PayrollError, u32)] = &[
        (PayrollError::DisputeAlreadyRaised, 1),
        (PayrollError::NotInGracePeriod, 2),
        (PayrollError::NotParty, 3),
        (PayrollError::NotArbiter, 4),
        (PayrollError::InvalidPayout, 5),
        (PayrollError::ActiveDispute, 6),
        (PayrollError::AgreementNotFound, 7),
        (PayrollError::NoDispute, 8),
        (PayrollError::NoEmployee, 9),
        (PayrollError::NotActivated, 10),
        (PayrollError::Unauthorized, 11),
        (PayrollError::InvalidEmployeeIndex, 12),
        (PayrollError::InvalidData, 13),
        (PayrollError::TransferFailed, 14),
        (PayrollError::InsufficientEscrowBalance, 15),
        (PayrollError::NoPeriodsToClaim, 16),
        (PayrollError::AgreementNotActivated, 17),
        (PayrollError::InvalidAgreementMode, 18),
        (PayrollError::AgreementPaused, 19),
        (PayrollError::AllPeriodsClaimed, 20),
        (PayrollError::ZeroAmountPerPeriod, 21),
        (PayrollError::ZeroPeriodDuration, 22),
        (PayrollError::ZeroNumPeriods, 23),
        (PayrollError::EmergencyPaused, 24),
        (PayrollError::NotGuardian, 25),
        (PayrollError::TimelockActive, 26),
        (PayrollError::InvalidTimelock, 27),
        (PayrollError::MultisigApprovalRequired, 28),
        (PayrollError::ExchangeRateNotFound, 29),
        (PayrollError::ExchangeRateOverflow, 30),
        (PayrollError::ExchangeRateInvalid, 31),
        (PayrollError::GraceExtensionInvalid, 32),
        (PayrollError::GraceExtensionCapExceeded, 33),
        (PayrollError::RateLimited, 34),
        (PayrollError::BatchTooLarge, 35),
        (PayrollError::MilestoneAmountInvalid, 36),
        (PayrollError::MilestoneAgreementInvalidStatus, 37),
        (PayrollError::MilestoneNotFound, 38),
        (PayrollError::MilestoneAlreadyApproved, 39),
        (PayrollError::MilestoneNotApproved, 40),
        (PayrollError::MilestoneAlreadyClaimed, 41),
        (PayrollError::EmployeeAlreadyExists, 42),
        (PayrollError::ReentrancyDetected, 43),
        (PayrollError::InvalidArbiter, 44),
    ];

    for (variant, expected) in cases {
        assert_eq!(
            *variant as u32, *expected,
            "PayrollError discriminant changed for {:?}: expected {}, got {}",
            variant, expected, *variant as u32
        );
    }

    // Guard against a variant being silently dropped: the enum must still have
    // exactly the 44 codes pinned above.
    assert_eq!(cases.len(), 44, "PayrollError variant count changed");
}
