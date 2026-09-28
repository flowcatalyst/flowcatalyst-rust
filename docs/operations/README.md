# Operations

Production deployment, configuration, and operational procedures for FlowCatalyst. Audience: infrastructure engineers, SREs, anyone running FlowCatalyst in production.

For internal architecture see [../architecture/](../architecture/). For building applications against the platform see [../developers/](../developers/).

## Start here

| If you're… | Read |
|---|---|
| Picking a deployment shape | [topologies.md](topologies.md) |
| Setting env vars for a specific binary | [configuration.md](configuration.md) |
| Provisioning the database | [postgres.md](postgres.md) |
| Provisioning the queue | [queue-and-router-config.md](queue-and-router-config.md) |
| Setting up active/standby HA | [high-availability.md](high-availability.md) |
| Handling secrets, rotation | [secrets-and-rotation.md](secrets-and-rotation.md) |
| Setting up an IDP (Entra, Keycloak, …) | [identity-and-auth.md](identity-and-auth.md) |
| Wiring up monitoring | [observability.md](observability.md) |
| Responding to an incident | [runbooks.md](runbooks.md) |
| Adding a dependency, reading a release's SBOM | [supply-chain.md](supply-chain.md) |

## At-a-glance checklist for a fresh production deploy

1. **Postgres**: provision, set credentials in your secret store, decide between `FC_DATABASE_URL` and AWS Secrets Manager mode. [postgres.md](postgres.md)
2. **SQS** (or alternative): create a FIFO queue, grant IAM access. [queue-and-router-config.md](queue-and-router-config.md)
3. **Redis**: provision if running HA. [high-availability.md](high-availability.md)
4. **Secrets**: JWT keys (RSA pair), `FLOWCATALYST_APP_KEY` (AES-256). [secrets-and-rotation.md](secrets-and-rotation.md)
5. **IDP** (optional but typical): register a FC client with your IDP, configure email-domain mapping, anchor domain. [identity-and-auth.md](identity-and-auth.md)
6. **Binary**: pick fc-server (most cases) or a split topology. [topologies.md](topologies.md)
7. **Env**: full configuration reference. [configuration.md](configuration.md)
8. **Health + metrics**: Prometheus scrape, k8s probes, dashboards. [observability.md](observability.md)
9. **Runbooks**: bookmark for incidents. [runbooks.md](runbooks.md)

## Operational philosophy

A few principles repeated throughout the docs:

- **One write path per aggregate.** Don't bypass the use case layer. CLAUDE.md (repo root) covers this.
- **Migrations are immutable once shipped.** Never edit; always add a new one. [postgres.md](postgres.md#migrations)
- **Operator pause beats hot config knobs.** When something's misbehaving, pause the connection (persistent) rather than tweaking config. [queue-and-router-config.md](queue-and-router-config.md#connections-and-pause)
- **Three independent leak-stoppers.** Stale recovery (scheduler), outbox recovery, lifecycle reaper (router). They run forever, ideally never doing anything. When they fire, investigate.
- **Same code in dev and prod.** Partition manager, migrations, leader election all run identically in fc-dev's embedded PG and in prod RDS.

If you're new to FlowCatalyst, the most useful thing to read after this directory is [../architecture/system-overview.md](../architecture/system-overview.md) — it gives a 10-minute mental model of how the parts fit together.
