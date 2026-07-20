#!/bin/bash
# Run this script with sudo to revert permissions configured for Jumpy on Linux.

if [ "$EUID" -ne 0 ]; then
  echo "Please run as root (using sudo)"
  exit 1
fi

ACTUAL_USER=${SUDO_USER:-$USER}

if [ -z "$ACTUAL_USER" ] || [ "$ACTUAL_USER" == "root" ]; then
    echo "Could not determine the original user. Please run with sudo as a normal user."
    exit 1
fi

echo "Reverting input permissions for user: $ACTUAL_USER"

# 1. Remove the user from the input group
if groups "$ACTUAL_USER" | grep &>/dev/null '\binput\b'; then
    gpasswd -d "$ACTUAL_USER" input
    echo "Removed $ACTUAL_USER from the 'input' group."
else
    echo "User $ACTUAL_USER is not in the 'input' group."
fi

# 2. Remove the udev rule
UDEV_RULE_FILE="/etc/udev/rules.d/99-jumpy-input.rules"
if [ -f "$UDEV_RULE_FILE" ]; then
    rm -f "$UDEV_RULE_FILE"
    echo "Removed udev rule at $UDEV_RULE_FILE"
    
    # Reload udev rules
    udevadm control --reload-rules
    udevadm trigger
    echo "Reloaded udev rules."
else
    echo "Udev rule $UDEV_RULE_FILE not found."
fi

echo ""

# 3. Close firewall ports (52637 and 52638 UDP)
echo "Closing firewall ports 52637 and 52638 (UDP)..."
if command -v ufw >/dev/null 2>&1; then
    ufw delete allow 52637/udp
    ufw delete allow 52638/udp
    echo "Closed ports using ufw."
elif command -v firewall-cmd >/dev/null 2>&1; then
    firewall-cmd --permanent --remove-port=52637/udp
    firewall-cmd --permanent --remove-port=52638/udp
    firewall-cmd --reload
    echo "Closed ports using firewalld."
elif command -v iptables >/dev/null 2>&1; then
    iptables -D INPUT -p udp --dport 52637 -j ACCEPT
    iptables -D INPUT -p udp --dport 52638 -j ACCEPT
    echo "Closed ports using iptables."
else
    echo "Warning: Could not find ufw, firewalld, or iptables. Please manually close UDP ports 52637 and 52638."
fi

echo ""
echo "=================================================================="
echo "Revert complete! IMPORTANT: You must log out and log back in"
echo "(or restart your computer) for the group changes to take effect."
echo "=================================================================="
