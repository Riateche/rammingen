# Rammingen server

Rammingen server handles database and file storage and provides API to the clients. **Both database and file storage should be backed up to protect against data loss.**

An instance of Rammingen server can only serve one user and provides them with full access to all stored files and metadata.

The user can run Rammingen clients on all their systems. Typically, one client is running on each PC or phone of the user. These clients should be connected to the same Rammingen server. It's recommended to create a separate `source` for each system where you'd like to run the client. You can assign a separate name and access token to each of the sources. In the version history, the name of the source will be displayed for every change of an entry.

Rammingen uses end-to-end encryption to store file content and metadata on the server. All file content and metadata (including file names) is encrypted before it's sent to the server. To decrypt it, an encryption key is required. The encryption key should only be accessible by Rammingen clients and should never leave their systems. (All clients must use the same encryption key.)

## Setting up

The [top level README](../README.md) contains a step-by-step guide for setting up Rammingen server on Ubuntu. This section covers advanced details and tips for setting up.

## Config path

Default config path for Rammingen server depends on the OS:

- Linux: `/etc/rammingen-server.conf`
- macOS: `$HOME/Library/Application Support/rammingen-server.conf`
- Windows: `%APPDATA%\rammingen-server.conf`

Note that when running Rammingen server in Docker, this refers to the path inside the container.

A different config path can be set using `--config` command line option.

## Admin tool

`rammingen-admin` is a command line tool that allows you to perform the following operations:

- List configured sources
- Add new source
- Update access token for a source
- Apply migrations to the database (required on first setup and after upgrading server version)
- Update server ID (required after restoring database from backup)

The Rammingen Docker image contains `rammingen-admin`.

## Building from source

Building from source allows you to run the server natively on your Linux system without Docker. It is also the recommended way to run the server on Windows and macOS.

First, install [rustup](https://rustup.rs/).

Then, to build rammingen-server and rammingen-admin, run the following commands:
```sh
git clone git@github.com:Riateche/rammingen.git
cd rammingen
git checkout 0.2.0-alpha.1
cargo build --release --locked --package rammingen_server
```
Output binaries (`rammingen-server` and `rammingen-admin`) will be created in `target/release`.

## Virtual file systems and database structure

All user files from all sources are arranged in a virtual filesystem tree. Files and directories within that tree are identified by **archive path** (`ar:/...`). Archive paths are mapped to local paths in Rammingen client config. Archive paths are not accessible on server. Instead, each archive path is converted to an encrypted representation - an **encrypted archive path** (`enar:/...`). A notable property of this representation is that it preserves parent-child relationship of paths. That allows the server to maintain important invariants and handle some high-level commands (e.g. move or remove a directory recursively) without knowing any real file names or directories.

The server stores the file metadata as entries and entry versions. An **entry** (stored in `entries` table) is the current state of a particular encrypted path in the virtual archive filesystem. (That means that it also corresponds to a certain non-encrypted archive path, but that path is not available to the server.) An entry can represent a directory, a file, or an absense of a previously existing directory or file. The directory's entry doesn't include its content - that is stored as separate entries. For any entry, all parent paths (up to the archive root) must correspond to an existing directory entry.

Every time an entry is created or updated, an **entry version** is created with the new properties of the entry. A list of entry versions corresponding to a certain entry represents a history of changes of the entry at a particular path. Entry versions are created using a PostgreSQL trigger.

## Snapshots and version history

Rammingen server stores all recent versions of all files (based on `retain_detailed_history_for` config option), so that any changes can be rolled back or inspected. However, it also provides a mechanism for cleaning up older information about versions.

Every once in a while (based on `snapshot_interval` config option), the server will create a snapshot of the current state of the virtual filesystem tree. A snapshot can be thought of as a lightweight copy of the state of all archive entries at a certain time.

For example, if the server config has `snapshot_interval: "1week", retain_detailed_history_for: "2weeks"`, the available history will be as follows:

* From distant past to 2 weeks ago - snapshots only (e.g. file versions at 2 weeks ago, 3 weeks ago, 4 weeks ago, etc.)
* From 2 weeks ago until now - all file versions.

Removing detailed history only happens when a snapshot is created, so actual time interval for which the detailed history is available may be larger than the configured value of `retain_detailed_history_for`.

Note that the list of changes for a file or directory will only display snapshots that actually contain changes compared to a previous snapshot.

## Restoring server from backup

To restore the Rammingen server from backup, follow these steps:

1. Shut down rammingen-server.
2. Restore the database and the local file storage from a backup.
3. Apply database migrations:
    ```sh
    docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        riateche/rammingen:0.2.0-alpha.1 migrate
    ```
4. Update server ID to ensure that clients will be working correctly:
    ```sh
    docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        riateche/rammingen:0.2.0-alpha.1 update-server-id
    ```
5. Start rammingen-server.

Next, for every client, repeat the following actions:

1. Run `rammingen clear-local-cache`.
2. For every configured local mount:
    1. If you'd like to restore these files from server, remove the local files. The first `rammingen sync` will download these files from server again.
    2. If you'd like to overwrite archive files with local files, keep the local files intact. The first `rammingen sync` will upload all existing local files as new changes.
3. Run `rammingen sync`.

## Development tips

### Updating sqlx query metadata

If any SQL queries are added or modified in the code, the build will fail until the sqlx files in `server/.sqlx` are updated.

First, install `sqlx-cli` if it's not installed yet:
```sh
cargo install sqlx-cli@0.8.6
```
Next, run a temporary database in Docker:
```sh
docker run --name rammingen_local \
    -e POSTGRES_HOST_AUTH_METHOD=trust \
    -p 6123:5432 \
    -d \
    postgres:alpine
```
Apply migrations to the database:
```sh
cargo run -p rammingen_server --bin rammingen-admin -- \
    --database-url postgres://postgres@127.0.0.1:6123/ \
    migrate
```
Update SQLX files:
```sh
export DATABASE_URL=postgres://postgres@127.0.0.1:6123/
cd server
cargo sqlx prepare
```
The files in `server/.sqlx` will be updated.

### Changing database structure

1. Add a migration to `server/migrations`.
2. Run autotests to recreate the database from migrations:
    ```sh
    docker rm -f rammingen_autotest
    docker run --rm --name rammingen_autotest \
        -e POSTGRES_HOST_AUTH_METHOD=trust -p 6123:5432 -d \
        postgres:alpine
    cargo run --bin rammingen_tests -- \
        --database-url postgres://postgres@127.0.0.1:6123/ shuffle
    ```
3. Update schema file:
    ```sh
    docker exec rammingen_autotest \
        pg_dump --user postgres --schema-only > server/schema.sql
    ```
4. When the server is updated, run migrations on the database:
    ```sh
    docker run \
        --volume /etc/rammingen-server.conf:/etc/rammingen-server.conf:ro \
        --entrypoint /sbin/rammingen-admin \
        riateche/rammingen:0.2.0-alpha.1 migrate
    ```
