# awsome

A small utility tool for AWS CLI dedicated to managing EC2 instance
state, SSO login and first time setup for devs.

It remembers one or more profile+instance pairs in a small config
file next to the executable, so day-to-day usage for login + starting
instances is just:

```sh
awsome
```

<img src="./demo.avif" />

For more details on available commands check out
[`cli.rs`](./src/cli.rs) or run the following:

```sh
awsome help
```

## Requirements

- [AWS CLI v2](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html)
- [Session Manager plugin for the AWS CLI](https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html)
- One or more AWS CLI profiles already configured

## Install (Windows x64)

```powershell
irm https://raw.githubusercontent.com/lfod26/awsome/main/scripts/install.ps1 | iex
```

### Uninstalling

1. Delete the install directory:
   ```powershell
   Remove-Item -Recurse -Force "$env:USERPROFILE\.awsome"
   ```
2. Remove `%USERPROFILE%\.awsome` from your PATH via System Settings.

---

Don't see your platform/arch? Raise an issue or build it from source via the Rust toolchain.
