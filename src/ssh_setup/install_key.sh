#!/bin/sh
# Installs an SSH public key into a user's authorized_keys. Sourced and called
# by awsome's `build_push_script`; runs remotely via SSM Run Command *as root*.
#
# Usage: install_key <user> <public-key>
#
# Since Run Command runs as root, a bare ~/.ssh would resolve to /root/.ssh;
# this resolves the target user's real home via getent and fixes
# ownership/permissions. The append is idempotent (grep -qF).
install_key() {
    user=$1
    pubkey=$2

    home_dir=$(getent passwd "$user" | cut -d: -f6)
    if [ -z "$home_dir" ]; then
        echo "could not resolve $user home directory" >&2
        return 1
    fi

    ssh_dir="$home_dir/.ssh"
    keys="$ssh_dir/authorized_keys"

    install -d -m 700 -o "$user" -g "$user" "$ssh_dir"
    touch "$keys"
    chmod 600 "$keys"
    chown "$user:$user" "$keys"

    if grep -qF "$pubkey" "$keys"; then
        echo 'Public key already present in authorized_keys.'
    else
        printf '%s\n' "$pubkey" >> "$keys"
        echo 'Public key added to authorized_keys.'
    fi
}
