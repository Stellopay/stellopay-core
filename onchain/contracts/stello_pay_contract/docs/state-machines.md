## Milestone State Machine

### Approval Transition
- **From**: `Submitted`
- **To**: `Approved`
- **Trigger**: `approve_milestone(milestone_id)`
- **Constraint**: The transaction **must** be signed by the `approver` address stored in the parent `Agreement`.
- **Security Note**: This prevents the "Beneficiary Self-Approval" attack where an employee could unilaterally unlock their own payments.
