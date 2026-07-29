# Integration Guide

## Available Functions

To initialize the contract, call `initialize`.
To check the current rate limit, call `check_and_consume`.
To update the global limit, call `set_global_limit`.

## Deprecated Functions

Do not use `old_deprecated_func`.
The function `never_existed` was removed in v2.
Use `get_limit_for` instead of `legacy_limit_lookup`.
