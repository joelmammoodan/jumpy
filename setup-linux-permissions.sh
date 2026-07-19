#!/bin/bash
# Run this script with sudo to configure permissions for Jumpy on Linux.

if [ "$EUID" -ne 0 ]; then
  echo "Please run as root (using sudo)"
  exit 1
fi

# 1. Determine the actual user who invoked sudo
ACTUAL_USER=${SUDO_USER:-$USER}

if [ -z "$ACTUAL_USER" ] || [ "$ACTUAL_USER" == "root" ]; then
    echo "Could not determine the original user. Please run with sudo as a normal user."
    exit 1
fi

echo "Setting up input permissions for user: $ACTUAL_USER"

# 2. Add the user to the input group
# (This allows them to read from /dev/input/*)
usermod -aG input "$ACTUAL_USER"
echo "Added $ACTUAL_USER to the 'input' group."

# 3. Create a udev rule to allow the input group to write to /dev/uinput
# (This is required for injecting virtual mouse/keyboard movements)
UDEV_RULE_FILE="/etc/udev/rules.d/99-jumpy-input.rules"
echo 'KERNEL=="uinput", MODE="0660", GROUP="input", OPTIONS+="static_node=uinput"' > "$UDEV_RULE_FILE"
echo "Created udev rule at $UDEV_RULE_FILE"

# 4. Reload udev rules
udevadm control --reload-rules
udevadm trigger
echo "Reloaded udev rules."

echo ""
echo "=================================================================="
echo "Setup complete! IMPORTANT: You must log out and log back in"
echo "(or restart your computer) for the group changes to take effect."
echo "After that, Jumpy will be able to capture input without sudo."
echo "=================================================================="
