# Deployment

Deployment is not implemented yet.

The planned control-plane deployment contains independently deployable SENSOR
Rendezvous, SENSOR Relay, SENSOR API, SENSOR Authentication, SENSOR Management
Backend, and SENSOR Update Service components. Development will use Docker
Compose; production will provide Kubernetes manifests, regional relays,
Prometheus-compatible metrics, structured logs, secret injection, and database
migrations.

No endpoint installer, Windows service, server image, or update package should
be described as deployable until it has signed artifacts and installation
tests.

