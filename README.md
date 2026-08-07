# awsome

`awsome` is a small AWS CLI companion for managing configured EC2 instances,
SSO login, scheduled shutdowns, and SSH access through AWS Systems Manager
(SSM).

It stores one or more AWS profile and EC2 instance pairs in
`awsome_conf.json` next to the executable (or in the current directory for a
debug build). One group is selected at a time; instance commands always use
that selected group.

## Configure an instance

Add the first group interactively:

```sh
awsome configure add
```

To avoid prompts, provide both values:

```sh
awsome configure add --profile development --instance-id i-0123456789abcdef0
```

List groups, select a target, or remove a non-selected group:

```sh
awsome configure show
awsome configure select --index 2
awsome configure remove --index 1
```

Indexes are 1-based, as shown by `configure show`. Omitting `--index` for
`select` or `remove` opens an interactive picker. `configure add` checks the
selected AWS profile and instance before saving; if its credentials are missing
or expired, it starts an interactive `aws sso login` flow.

## Manage the selected instance

Run `awsome` with no subcommand to start the selected instance and wait for it
to become `running`:

```sh
awsome
```

The same operation can be explicit, and an in-instance shutdown can be
scheduled for a local 24-hour time:

```sh
awsome start
awsome start --schedule-shutdown 18:30
awsome stop
```

Scheduled shutdown uses SSM Run Command. If a shutdown is already pending on
the instance, it is left unchanged.

## SSH access

`awsome setup-ssh` prepares SSH access to the currently selected instance:

1. ensures a dedicated local `ed25519` key at `~/.ssh/awsome`;
2. installs its public key for the remote `ec2-user` through SSM Run Command;
3. creates or updates an `awsome`-managed `Host awsome` entry in
   `~/.ssh/config`.

```sh
awsome setup-ssh
ssh awsome
```

The managed host's `ProxyCommand` invokes `awsome ssh-proxy %p`, which opens an
SSH-over-SSM Session Manager tunnel. No public IP address or inbound port 22 is
required. The alias follows whichever profile/instance group is selected, so
switch targets with `awsome configure select` without editing SSH config.
`ssh-proxy` is an internal command used by OpenSSH and should not normally be
run directly.

## Requirements

- [AWS CLI v2](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html)
  installed and available on `PATH`.
- One or more AWS CLI profiles, configured with `aws configure --profile
  <name>` or SSO, with permission to describe, start, and stop the target EC2
  instances.
- For scheduled shutdowns and SSH setup, permission to use SSM Run Command
  against the target instance.
- For SSH setup, an OpenSSH client on `PATH` (`ssh-keygen` and `ssh`) and the
  AWS CLI [Session Manager plugin](https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html).
- For SSM-backed operations, the target instance must run the SSM Agent and
  have an instance profile that permits Systems Manager access.

Run `awsome --help` or `awsome <command> --help` for the complete command
reference.

## Download

Pre-built binaries are published on the
[**latest release**](https://github.com/lfod26/awsome/releases/latest)
page. Grab the asset for your platform:

Currently the build is done for:

- Windows (x86_64)
- macOS (Apple Silicon / ARM)

Don't see your platform/arch? Raise an issue or build it from source via the Rust toolchain.
