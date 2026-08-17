# Payment Scheduling System

## Overview
The Stellopay Payment Scheduler allows employers to programmatically schedule future token transfers to recipients. It acts as an on-chain cron system, processing jobs via the `process_due_payments` crank mechanism.



## Features
* **Recurring Payments:** Jobs can execute infinitely or be capped via `max_executions`. Intervals are defined in seconds.
* **One-Time Payments:** By setting `max_executions` to `1`, employers can schedule single delayed transfers. The `interval_seconds` is ignored and can safely be set to `0`.
* **Conflict Detection:** To prevent accidental double-billing (e.g., UI double-clicks or API retries), the contract prevents the creation of a schedule if an active job already exists for the exact same `(Employer, Recipient, Start Time)`.
* **Resilience:** Jobs that fail due to insufficient employer balances in the contract will automatically increment a `retry_count` and reschedule themselves until the `max_retries` ceiling is hit.
* **Control:** Employers can independently `pause_job` and `resume_job` at any point in the schedule's lifecycle.

## Workflow
1. **Initialize:** Contract is initialized by the protocol admin.
2. **Fund:** The employer transfers tokens into the Scheduler contract via `fund_job` (or via direct Soroban token transfers).
3. **Schedule:** Employer calls `create_job` detailing the schedule parameters, including the first execution timestamp.
4. **Execute:** An external keeper/crank periodically calls `process_due_payments`, which iterates through active jobs and releases funds to recipients whose `next_scheduled_time` has elapsed.