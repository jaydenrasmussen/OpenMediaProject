#!/bin/bash
# Starts the app, and Gluetun first when a VPN is configured.
#
# No VPN_SERVICE_PROVIDER: the app runs as the unprivileged `omp` user, as
# before. With one: Gluetun brings the tunnel up in this container and its
# firewall blocks everything outside it, so every connection the app makes
# (indexers, usenet, torrents, metadata) goes through the VPN. The app's own
# port stays reachable.
set -euo pipefail

APP=/usr/local/bin/omp-server

# A VPN set up on the Settings page is kept in vpn.env in the data folder.
# The container's own environment wins: only what is not already set is read.
VPN_FILE="${OMP_DATA_DIR:-/config}/vpn.env"
if [ -z "${VPN_SERVICE_PROVIDER:-}" ] && [ -r "$VPN_FILE" ]; then
  while IFS='=' read -r key value; do
    case "$key" in
      VPN_SERVICE_PROVIDER|VPN_TYPE|OPENVPN_USER|OPENVPN_PASSWORD|WIREGUARD_PRIVATE_KEY|WIREGUARD_ADDRESSES|SERVER_COUNTRIES|SERVER_CITIES|SERVER_HOSTNAMES|FIREWALL_OUTBOUND_SUBNETS)
        [ -z "${!key+x}" ] && export "$key=$value" ;;
    esac
  done < "$VPN_FILE"
  export OMP_VPN_FROM_FILE=1
fi
PORT="${OMP_ADDR##*:}"
PORT="${PORT:-8080}"

# What the app must not see: the VPN account.
SECRETS=(-u OPENVPN_USER -u OPENVPN_PASSWORD -u WIREGUARD_PRIVATE_KEY -u WIREGUARD_PRESHARED_KEY
  -u WIREGUARD_ADDRESSES -u AMNEZIAWG_PRIVATE_KEY -u AMNEZIAWG_PRESHARED_KEY
  -u HTTPPROXY_PASSWORD -u SHADOWSOCKS_PASSWORD -u HTTP_CONTROL_SERVER_AUTH_DEFAULT_ROLE)

run_app() {
  if [ "$(id -u)" = 0 ]; then
    exec setpriv --reuid=omp --regid=omp --init-groups env HOME=/home/omp "$@"
  fi
  exec env "$@"
}

if [ -z "${VPN_SERVICE_PROVIDER:-}" ] || [ "$(id -u)" != 0 ]; then
  if [ -n "${VPN_SERVICE_PROVIDER:-}" ]; then
    echo "VPN_SERVICE_PROVIDER is set but the container is not running as root, so the VPN cannot start." >&2
    exit 1
  fi
  run_app "$APP" "$@"
fi

if [ ! -c /dev/net/tun ]; then
  echo "The VPN needs the tunnel device: run the container with --device /dev/net/tun --cap-add NET_ADMIN." >&2
  exit 1
fi

# The control server answers on loopback only, and only the two read-only
# routes the app uses are open without a login.
export HTTP_CONTROL_SERVER_ADDRESS="${HTTP_CONTROL_SERVER_ADDRESS:-127.0.0.1:8000}"
mkdir -p /gluetun/auth
cat > /gluetun/auth/config.toml <<'TOML'
[[roles]]
name = "omp"
auth = "none"
routes = ["GET /v1/vpn/status", "GET /v1/publicip/ip"]
TOML
export OMP_GLUETUN_URL="http://${HTTP_CONTROL_SERVER_ADDRESS}"
export FIREWALL_INPUT_PORTS="${FIREWALL_INPUT_PORTS:+$FIREWALL_INPUT_PORTS,}${PORT}"

# Gluetun's own defaults, for anything the user did not set.
(
  while IFS='=' read -r key value; do
    [ -n "$key" ] && [ -z "${!key+x}" ] && export "$key=$value"
  done < /etc/gluetun.defaults
  exec /usr/local/bin/gluetun-entrypoint
) &
GLUETUN=$!

# Give the tunnel a minute; if it is slow the app still starts and shows a
# health warning until it is up.
for _ in $(seq 1 60); do
  kill -0 "$GLUETUN" 2>/dev/null || { echo "Gluetun exited." >&2; exit 1; }
  if exec 3<>"/dev/tcp/127.0.0.1/${HTTP_CONTROL_SERVER_ADDRESS##*:}" 2>/dev/null; then
    printf 'GET /v1/vpn/status HTTP/1.0\r\n\r\n' >&3
    if grep -q '"running"' <&3; then exec 3>&-; break; fi
    exec 3>&-
  fi
  sleep 1
done

( run_app env "${SECRETS[@]}" "$APP" "$@" ) &
APPPID=$!

trap 'kill -TERM "$APPPID" "$GLUETUN" 2>/dev/null' TERM INT
# Either one ending takes the container down, so the restart policy sees it.
set +e
wait -n "$APPPID" "$GLUETUN"
CODE=$?
kill -TERM "$APPPID" "$GLUETUN" 2>/dev/null
wait 2>/dev/null
exit "$CODE"
