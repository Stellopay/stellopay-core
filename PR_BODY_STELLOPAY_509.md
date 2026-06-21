## Summary
Cap maximum metadata length in log_record in compliance_reporting to bound storage growth.

## Changes
- Added MAX_METADATA_LENGTH constant (2048 bytes) with doc comment
- Added MetadataTooLong error variant (ComplianceError::MetadataTooLong = 8)
- Added metadata length validation in log_record: rejects metadata.len() > MAX_METADATA_LENGTH before storage write
- Added 3 boundary tests: empty (pass), max (pass), over-max (rejected with MetadataTooLong)
- Updated docs/compliance-reporting-schema.md with metadata length limit documentation
- Fixed test file: added missing function header for 	est_get_withholding_records_empty_result (pre-existing compile error)

## Verification
- git diff --check passes
- All existing tests continue to pass (existing records use empty metadata, len=0)
- Boundary: empty metadata (0 bytes) → accepted
- Boundary: max metadata (2048 bytes) → accepted
- Boundary: over-max metadata (2049 bytes) → rejected with MetadataTooLong

## Closes
Closes #509

## Wallet
RTC269fa5650798c3aa5086a128c025a546e0a41d0b
