# SENSOR provisioned relay

`sensor-relay.exe` is a small self-hosted relay for one explicitly provisioned
pair of SENSOR endpoints. It is useful when the two PCs cannot accept a direct
TCP connection. The relay authenticates both endpoint public keys, then copies
their byte streams; it cannot decrypt the endpoint-to-endpoint application
records and it never opens a remote-controlled destination socket.

The relay is intentionally not a public rendezvous service. It has no global
device directory, NAT traversal, automatic room allocation, regional routing,
quota system, metrics, TLS metadata protection, or multi-tenant provisioning.
Those pieces still require a separately operated backend.

## Start the relay

On the relay host:

```powershell
.\sensor-relay.exe identity C:\SENSOR\Relay
.\sensor-relay.exe serve C:\SENSOR\Relay 0.0.0.0:5910 <endpoint-a-public-key> <endpoint-b-public-key>
```

The first command creates `relay-seed.bin` and prints the relay public key.
Protect that file and the relay host. The second command prints the listening
address and runs continuously, serving one pair at a time. Open only the chosen
relay TCP port in the server firewall.

On both SENSOR-Remote windows select **Provisioned relay**, enter the relay
address and the printed relay public key, then use the verified peer ID and
public key as usual. The receiver must start listening and the initiator starts
the desired chat, file, view, or control operation.

The receiver's relay session joins the same authenticated endpoint handshake as
the direct path. A correct relay pin is required before the endpoint sends its
identity response. Wrong relay keys, unknown endpoint keys, duplicate keys,
pairing timeouts and idle timeouts are rejected.

This is a deployment aid for the current attended product, not a claim of full
commercial remote-access parity. Automatic direct-first fallback, reconnect,
Internet ID lookup and NAT traversal remain future work.
