> ## Documentation Index
>
> Fetch the complete documentation index at: [/llms.txt](https://docs.turso.tech/llms.txt)
>
> Use this file to discover all available pages before exploring further.

[Skip to main content](https://docs.turso.tech/sdk/rust/quickstart#content-area)

[Turso home page![light logo](https://mintcdn.com/turso/0gN7SecUGL1uVO9v/logo/turso-light.svg?fit=max&auto=format&n=0gN7SecUGL1uVO9v&q=85&s=089d09358d8f5a3e3815861a7419a616)![dark logo](https://mintcdn.com/turso/0gN7SecUGL1uVO9v/logo/turso-dark.svg?fit=max&auto=format&n=0gN7SecUGL1uVO9v&q=85&s=2946b82b1620bb2c7839f85d7e2e113b)](https://docs.turso.tech/)

Search...

Ctrl K

- [Dashboard](https://turso.tech/app)
- [tursodatabase/turso\\
\\
24,398](https://github.com/tursodatabase/turso "tursodatabase/turso")
- [tursodatabase/turso\\
\\
24,398](https://github.com/tursodatabase/turso "tursodatabase/turso")

Search...

Navigation

Rust

Turso Quickstart (Rust)

[Turso Database](https://docs.turso.tech/introduction) [Turso Cloud](https://docs.turso.tech/turso-cloud) [AgentFS (beta)](https://docs.turso.tech/agentfs/introduction)

- [Turso Homepage](https://turso.tech/)
- [Discord](https://tur.so/discord)
- [GitHub](https://github.com/tursodatabase/turso)

### Turso Cloud

- [Introduction](https://docs.turso.tech/turso-cloud)
- [Quickstart](https://docs.turso.tech/quickstart)
- [Local Development](https://docs.turso.tech/local-development)
- [libSQL](https://docs.turso.tech/libsql)
- [Limitations](https://docs.turso.tech/cloud/limitations)
- [Migrate to Turso](https://docs.turso.tech/cloud/migrate-to-turso)
- [Durability Guarantees](https://docs.turso.tech/cloud/durability)
- [Private Endpoints](https://docs.turso.tech/cloud/private-endpoints)
- [Database Access Allow Rules](https://docs.turso.tech/cloud/allow-rules)
- [BYOK Encryption](https://docs.turso.tech/cloud/encryption)
- [Usage & Billing](https://docs.turso.tech/help/usage-and-billing)
- [AI & Embeddings](https://docs.turso.tech/features/ai-and-embeddings)
- Embedded Replicas

- [Branching](https://docs.turso.tech/features/branching)
- [Point-in-Time Recovery](https://docs.turso.tech/features/point-in-time-recovery)
- [Recover Deleted Databases](https://docs.turso.tech/features/recover-deleted-databases)
- [SQLite Extensions](https://docs.turso.tech/features/sqlite-extensions)
- Deprecated


### SDKs

- [Introduction](https://docs.turso.tech/sdk/introduction)
- [Authentication](https://docs.turso.tech/sdk/authentication)
- Authorization

- SQL over HTTP

- Official SDKs



  - TypeScript

  - Rust



    - [Quickstart](https://docs.turso.tech/sdk/rust/quickstart)
    - [Reference](https://docs.turso.tech/sdk/rust/reference)
    - [Examples](https://github.com/tursodatabase/libsql/tree/main/libsql/examples)
    - Guides

    - ORMs
  - Go

  - Python

  - PHP

  - Ruby

  - ActiveRecord

  - Android

  - Swift

  - C
- Community SDKs


### CLI

- [Introduction](https://docs.turso.tech/cli/introduction)
- [Installation](https://docs.turso.tech/cli/installation)
- [Authentication](https://docs.turso.tech/cli/authentication)
- [Upgrading](https://docs.turso.tech/cli/upgrading)
- [Help](https://docs.turso.tech/cli/help)
- [Headless Mode](https://docs.turso.tech/cli/headless-mode)
- Commands


### API Reference

- [Introduction](https://docs.turso.tech/api-reference/introduction)
- [Quickstart](https://docs.turso.tech/api-reference/quickstart)
- [Authentication](https://docs.turso.tech/api-reference/authentication)
- [Errors](https://docs.turso.tech/api-reference/response-codes)
- Resources


### Integrations

- [MCP (AI agents)](https://docs.turso.tech/integrations/mcp)
- [Vercel](https://docs.turso.tech/integrations/vercel)

## On this page

- [Recommended: turso (Local + Cloud Sync)](https://docs.turso.tech/sdk/rust/quickstart#recommended-turso-local-%2B-cloud-sync)
- [Remote Access (Over-the-Wire)](https://docs.turso.tech/sdk/rust/quickstart#remote-access-over-the-wire)
  - [Remote Turso database: turso\_serverless](https://docs.turso.tech/sdk/rust/quickstart#remote-turso-database-turso_serverless)
  - [Remote libSQL database: libsql](https://docs.turso.tech/sdk/rust/quickstart#remote-libsql-database-libsql)
- [Using an ORM](https://docs.turso.tech/sdk/rust/quickstart#using-an-orm)
- [Embedded Replicas (libsql)](https://docs.turso.tech/sdk/rust/quickstart#embedded-replicas-libsql)

Rust

# Turso Quickstart (Rust)

Copy pageCopy page

Get started with Turso and Rust in a few simple steps.

Copy pageCopy page

In this Rust quickstart we will learn how to:

- Install the Turso crate
- Connect to a local or remote database
- Execute a query using SQL
- Sync changes to the cloud

## [​](https://docs.turso.tech/sdk/rust/quickstart\#recommended-turso-local-+-cloud-sync)  Recommended: turso (Local + Cloud Sync)

`turso` is the recommended crate for running a local database, including synchronizing it to and from Turso Cloud. It is built on the Turso Database engine — a ground-up rewrite of SQLite with concurrent writes (MVCC), async I/O, and native Rust async/await support.

1

Install

```
cargo add turso tokio --features tokio/full
```

2

Connect

```
use turso::Builder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Builder::new_local("app.db").build().await?;
    let conn = db.connect()?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL
        )",
        (),
    ).await?;

    conn.execute("INSERT INTO users (name) VALUES (?)", ("Alice",)).await?;

    let mut rows = conn.query("SELECT * FROM users", ()).await?;
    while let Some(row) = rows.next().await? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        println!("User: {} {}", id, name);
    }

    Ok(())
}
```

3

Sync (push and pull)

If you need to sync your local database with Turso Cloud, enable the `sync` feature:

```
cargo add turso --features sync
```

```
use turso::sync::Builder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Builder::new_remote("app.db")
        .with_remote_url(&std::env::var("TURSO_DATABASE_URL")?)
        .with_auth_token(&std::env::var("TURSO_AUTH_TOKEN")?)
        .build()
        .await?;

    let conn = db.connect().await?;

    conn.execute("INSERT INTO users (name) VALUES (?)", ("Bob",)).await?;

    // Push local writes to Turso Cloud
    db.push().await?;

    // Pull remote changes to local database
    db.pull().await?;

    Ok(())
}
```

All reads and writes happen against the local database file — fast, offline-capable. `push()` sends your changes to the cloud. `pull()` brings remote changes down. See the [reference](https://docs.turso.tech/sdk/rust/reference) for checkpoint, stats, and encryption. See [Turso Sync](https://docs.turso.tech/sync/usage) for details on conflict resolution and more.

You can test sync locally without a Turso Cloud account by starting a local sync server:

```
tursodb :memory: --sync-server 127.0.0.1:8080
```

Then use `http://127.0.0.1:8080` as the remote URL (no auth token needed). See [Turso Database quickstart](https://docs.turso.tech/tursodb/quickstart) for how to install `tursodb`.

## [​](https://docs.turso.tech/sdk/rust/quickstart\#remote-access-over-the-wire)  Remote Access (Over-the-Wire)

If your application needs to query a Turso Cloud database directly over the network (e.g., from a web server or serverless function), use the crate that matches your database engine: `turso_serverless` for [Turso databases](https://docs.turso.tech/tursodb/quickstart), or `libsql` (with the `remote` feature) for [libSQL](https://docs.turso.tech/libsql) databases.

For most applications, we recommend running a local database with sync (`turso::sync`) instead — it gives you faster reads, offline support, and lower latency. Remote access is useful when you cannot store a local database file (e.g., stateless serverless environments).

### [​](https://docs.turso.tech/sdk/rust/quickstart\#remote-turso-database-turso_serverless)  Remote Turso database: turso\_serverless

`turso_serverless` connects to a remote Turso database over HTTP — no persistent connections, no C compiler required. Its API mirrors the embedded `turso` crate, so code moves between local and remote with minimal changes.

1

Retrieve database credentials

You will need an existing Turso database to continue. If you don’t have one, create one with `turso db create --tursodb` (see the [quickstart](https://docs.turso.tech/quickstart)).Get the database URL:

```
turso db show --url <database-name>
```

Get the database authentication token:

```
turso db tokens create <database-name>
```

Assign credentials to the environment variables inside `.env`.

```
TURSO_DATABASE_URL=turso://[databaseName]-[organizationSlug].turso.io
TURSO_AUTH_TOKEN=
```

You will want to store these as environment variables.

2

Install

```
cargo add turso_serverless tokio --features tokio/full
```

3

Connect and query

```
use turso_serverless::Builder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = Builder::new_remote(std::env::var("TURSO_DATABASE_URL")?)
        .with_auth_token(std::env::var("TURSO_AUTH_TOKEN")?)
        .build()
        .await?;
    let conn = db.connect()?;

    let mut rows = conn.query("SELECT * FROM users", ()).await?;
    while let Some(row) = rows.next().await? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        println!("User: {} {}", id, name);
    }

    Ok(())
}
```

### [​](https://docs.turso.tech/sdk/rust/quickstart\#remote-libsql-database-libsql)  Remote libSQL database: libsql

The `libsql` crate with the `remote` feature connects to a remote libSQL database over HTTP — no local file needed, no C compiler required.

1

Retrieve database credentials

You will need an existing libSQL database to continue. If you don’t have one, [create one](https://docs.turso.tech/quickstart).Get the database URL:

```
turso db show --url <database-name>
```

Get the database authentication token:

```
turso db tokens create <database-name>
```

Assign credentials to the environment variables inside `.env`.

```
TURSO_DATABASE_URL=
TURSO_AUTH_TOKEN=
```

You will want to store these as environment variables.

2

Install

```
cargo add libsql --features remote
```

3

Connect and query

```
use libsql::Builder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("TURSO_DATABASE_URL")?;
    let token = std::env::var("TURSO_AUTH_TOKEN")?;

    let db = Builder::new_remote(url, token).build().await?;
    let conn = db.connect()?;

    let mut rows = conn.query("SELECT * FROM users", ()).await?;
    while let Some(row) = rows.next().await? {
        let id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        println!("User: {} {}", id, name);
    }

    Ok(())
}
```

## [​](https://docs.turso.tech/sdk/rust/quickstart\#using-an-orm)  Using an ORM

[Toasty](https://docs.turso.tech/sdk/rust/orm/toasty), the async ORM from the Tokio project, has a native Turso driver. It speaks to the `turso` crate directly, so you get a typed model layer (`#[derive(toasty::Model)]`, derived queries, relations) over the same Turso Database engine used in the quickstart above.

```
cargo add toasty --features turso
```

See the [Toasty + Turso guide](https://docs.turso.tech/sdk/rust/orm/toasty) for the full walkthrough.

## [​](https://docs.turso.tech/sdk/rust/quickstart\#embedded-replicas-libsql)  Embedded Replicas (libsql)

Embedded Replicas give your Rust app a local read copy of a Turso Cloud database. Reads are served locally; writes go to the cloud primary and are reflected back to the replica. Embedded Replicas are fully supported in production.For new projects that need sync, we recommend the `turso` crate with `turso::sync`: both reads and writes are local, you sync explicitly with `push()` / `pull()`, and the wire format is logical change-data-capture rather than page frames ( [benchmark](https://turso.tech/blog/sync-benchmark)).

See the [reference](https://docs.turso.tech/sdk/rust/reference) for full documentation on Embedded Replicas with `libsql`.

Was this page helpful?

YesNo

[Suggest edits](https://github.com/tursodatabase/turso-docs/edit/main/sdk/rust/quickstart.mdx) [Raise issue](https://github.com/tursodatabase/turso-docs/issues/new?title=Issue%20on%20docs&body=Path:%20/sdk/rust/quickstart)

[Sentry](https://docs.turso.tech/sdk/ts/integrations/sentry) [Reference](https://docs.turso.tech/sdk/rust/reference)

[discord](https://tur.so/discord) [github](https://github.com/tursodatabase) [twitter](https://twitter.com/tursodatabase) [linkedin](https://www.linkedin.com/company/turso)

[Powered byThis documentation is built and hosted on Mintlify, a developer documentation platform](https://www.mintlify.com/?utm_campaign=poweredBy&utm_medium=referral&utm_source=turso)