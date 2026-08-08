# awsome

A small utility tool for AWS CLI dedicated to managing EC2 instance
state, SSO login and first time setup for devs.

It remembers one or more profile+instance pairs in a small config
file next to the executable, so day-to-day usage for login + starting
instances is just:

```sh
awsome
```

For more details on available commands check out
[`cli.rs`](./src/cli.rs) or run the following:

```sh
awsome help
```

## Requirements

- The [AWS CLI v2](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html)
  installed and available on `PATH`.
- One or more AWS CLI profiles already configured (`aws configure
--profile <name>`, or SSO) with permission to describe/start/stop the
  target EC2 instance(s).

## Download

Pre-built binaries are published on the
[**latest release**](https://github.com/lfod26/awsome/releases/latest)
page. Grab the asset for your platform:

Currently the build is done for:

- Windows (x86_64)
- macOS (Apple Silicon / ARM)

Don't see your platform/arch? Raise an issue or build it from source via the Rust toolchain.
