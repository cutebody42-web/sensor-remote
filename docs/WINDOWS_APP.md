# Using the native Windows app

SENSOR-Remote.exe is the graphical application. SENSOR-CLI.exe is an optional
console companion. Neither is a website. This is version 0.3.0 development,
with attended remote desktop, an explicitly provisioned relay path, and a
temporary Render Internet path implemented, but not the completed
commercial-equivalent product.

## First launch

1. Open SENSOR-Remote.exe. It runs as the current user, without elevation.
2. Your persistent device ID appears. Copy the public key to the other device
   owner through a trusted channel and compare the full key.
3. Without Render configuration it is normal to see **Offline — No listener
   started**. With `SENSOR_MODE=RENDER_TEST` and `SENSOR_SERVER` configured,
   the app registers an attended Internet listener while the window is open.

The supplied JPEG logo is embedded unchanged; no extra image file or browser
runtime is needed. Window rendering uses native egui/wgpu Direct3D 12.

## Two-PC attended chat

1. Exchange both device IDs and public keys. On each PC enter the other peer's
   ID/key and confirm that you compared it.
2. On the receiving PC, leave the default `0.0.0.0:5909` to listen on its
   network interfaces (or choose a specific local address), then press Start
   listening. `127.0.0.1` accepts connections from this PC only.
3. On the initiating PC enter the receiver's IP:port and Start encrypted chat.
4. The receiver sees the authenticated ID/key and requested mode. Accept or
   Reject explicitly. No unattended grant exists.
5. Chat is turn-based in this build: send, receive a reply, then send again.
   Reply/idle timeout is 120 seconds. Stop/disconnect closes the socket.

For a connection across networks, run `sensor-relay.exe` on a server and
provision both endpoint public keys. Select **Provisioned relay** on both SENSOR
windows and enter the relay address and relay public key. The relay forwards
endpoint-encrypted bytes; it does not replace endpoint authentication. See
[RELAY.md](RELAY.md).

No firewall configuration is automatic. Network/firewall setup for an actual
second PC is the device owner's responsibility. For Internet access, select
**Render test (HTTPS/WSS)** or configure the environment variables below; no
LAN address or port-forwarding is required.

## Render Internet mode

Deploy the temporary service using [the Render guide](../deployment/render/README.md),
then configure the client before launching:

```powershell
$env:SENSOR_MODE = 'RENDER_TEST'
$env:SENSOR_SERVER = 'https://<your-render-service>.onrender.com'
.\SENSOR-Remote.exe
```

The app derives the WSS endpoint, registers the device with a signed
challenge, retries during Render cold starts, and sends heartbeats while it is
waiting for a peer. A receiver shows the normal visible SENSOR consent dialog
for every incoming chat, file, view, or control request. Compare and save the
peer's full public key after the first approved connection.

## File transfer

Choose the receiving folder under File transfer on the receiver before starting
its listener. On the sender choose a file, leave Resume ID empty for a new
transfer and press Send file. The receiver must accept FileTransfer mode.

The receive root must exist. Each offer is limited to 1 GiB. Flat Windows-safe
names, 64 KiB chunk checksums and the final SHA-256 are verified. Existing target
files are refused, not overwritten. A successful completion message follows
the receiver's final integrity check and publication.

An interruption retains an incomplete private transfer in that receive root.
Keep the displayed transfer ID. Restart the receiver's listener, select the same
source file and enter the ID to resume; another acceptance is required. Do not
delete partial files if you intend to resume. There is no background retry.

## Attended remote desktop

On the receiver, press Start listening from Connect. On the initiating PC,
exchange and confirm the receiver's ID/key, then choose View remote desktop or
Control remote desktop. The receiver must explicitly approve the corresponding
request. View sends no keyboard or mouse input; Control enables the remote
viewport's pointer, buttons, wheel, text and key events. Leaving the viewport,
changing pages or pressing Stop sends ReleaseAll and closes the session safely.

The receiver captures only the ordinary unlocked `Default` interactive desktop.
The path uses DXGI Desktop Duplication and Windows Media Foundation H.264. It
does not unlock, switch to or bypass the secure desktop. A changed display mode,
desktop lock or UIPI/elevation refusal ends the session with an honest error.
The current build has not yet completed a real two-computer run on this locked
test machine.

If the receiver explicitly checks **Auto-accept this pinned peer while SENSOR
is open**, the verified peer can start an approved mode without an interactive
consent dialog. This is an attended-user process setting only: it is not a
Windows service, does not survive closing the app, and does not provide login
screen or UAC control.

## Contacts, aliases and audit

Trusted devices saves verified public pins and IP addresses locally. Saving an
existing ID does not silently replace its pinned key. Device & diagnostics can
save a local alias and verify incoming-session audit signatures and chain links.
Audit contains operation metadata, not chat or file content. Its displayed chain
head needs a separate trusted checkpoint to detect deletion of the entire tail.

Data is under %LOCALAPPDATA%/SENSOR Technology/Remote. To isolate a test profile:

```powershell
.\SENSOR-Remote.exe --config C:\SensorTest\ProfileA
```

Identity keys remain tied to the current Windows user by DPAPI. A profile is not
a cross-account portable credential backup. A second GUI for the same profile
is refused. Nothing starts with Windows, and closing the app stops its work.

## Not available yet

UAC/login/installed unattended service, H.265/AV1, audio, clipboard,
printing/Auto Print, recording, VPN, Internet ID lookup, NAT/route failover,
signed installer and signed updates are absent. See KNOWN_LIMITATIONS.md.
