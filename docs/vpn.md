# Built-in VPN

The container includes [Gluetun](https://github.com/qdm12/gluetun). Set `VPN_SERVICE_PROVIDER` and it brings up a VPN inside the container before the app starts. Gluetun's firewall blocks everything that is not in the tunnel, so every connection the app makes (indexers, usenet, torrents, metadata) goes through the VPN. If the tunnel drops, traffic stops; it never falls back to your own address.

With no VPN set up the container behaves as before and no VPN is started.

## Setting it up

There are two ways, and the environment wins if both are used.

- **In the app:** Settings, VPN. Pick the provider, fill in the account details, save, and restart. The settings are kept in `vpn.env` in the config folder (readable only by its owner), which the container reads when it starts. The page never shows a saved password or key again; leave the field empty to keep it.
- **With environment variables:** `VPN_SERVICE_PROVIDER` and the rest, as in the examples below. When these are set the Settings page shows the VPN as read-only.

A change needs a restart, and the container needs a restart policy (`--restart unless-stopped`) so it comes back.

## What the container needs

```
--cap-add NET_ADMIN --device /dev/net/tun
```

The container starts as root so Gluetun can create the tunnel. The app itself still runs as the unprivileged `omp` user (uid 1000), and it never sees your VPN account: the VPN variables are removed from its environment.

## Providers

Gluetun supports AirVPN, CyberGhost, ExpressVPN, FastestVPN, Giganews, HideMyAss, IPVanish, IVPN, Mullvad, NordVPN, Perfect Privacy, Privado, Private Internet Access, PrivateVPN, ProtonVPN, PureVPN, SlickVPN, Surfshark, TorGuard, VPN Secure, VPN Unlimited, VyprVPN, Windscribe, and any WireGuard or OpenVPN config of your own (`custom`). Gluetun's [wiki](https://github.com/qdm12/gluetun-wiki) has the variables for each provider; every Gluetun variable works here.

### Mullvad (WireGuard)

```yaml
services:
  omp:
    image: omp
    cap_add: [NET_ADMIN]
    devices: [/dev/net/tun:/dev/net/tun]
    ports: ["8080:8080"]
    environment:
      VPN_SERVICE_PROVIDER: mullvad
      VPN_TYPE: wireguard
      WIREGUARD_PRIVATE_KEY: <your key>
      WIREGUARD_ADDRESSES: 10.64.222.21/32
      SERVER_COUNTRIES: Sweden
    volumes: ["./config:/config", "./downloads:/downloads", "./media:/media"]
```

### NordVPN (OpenVPN)

Use the *service credentials* from the Nord account dashboard, not your login.

```yaml
    environment:
      VPN_SERVICE_PROVIDER: nordvpn
      OPENVPN_USER: <service username>
      OPENVPN_PASSWORD: <service password>
      SERVER_COUNTRIES: Netherlands
```

## Reaching other things on your network

The firewall also blocks your LAN. To let the app reach a Plex or Jellyfin server, or a download client, on the local network, allow that subnet:

```
FIREWALL_OUTBOUND_SUBNETS=192.168.1.0/24
```

The app's own port (from `OMP_ADDR`, 8080 by default) is opened for inbound connections automatically, so the web UI stays reachable. Add other inbound ports to `FIREWALL_INPUT_PORTS`.

## In the app

System shows a VPN section when the VPN is on: whether it is connected, the provider, and the address and place your traffic leaves from. While the tunnel is down, Health reports an error.

Gluetun's control server listens on `127.0.0.1:8000` inside the container only. Two read-only routes (status and public address) are open to the app; everything else needs Gluetun's own authentication.

## Notes

- OpenVPN is version 2.6. `OPENVPN_VERSION=2.5` is accepted but runs 2.6.
- Pin a Gluetun version by rebuilding with a different tag in the `gluetun` stage of the Dockerfile; its server lists are inside the binary, so a newer release has newer servers.
