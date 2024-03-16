# Rammingen

Rammingen is a self-hosted backup encryption system.

1. Syncs backed-up directories across several hosts.
1. Manages previous versions of backups.

Rammingen installation dependency graph (arrows represent the "X depends on Y" relationship):

```mermaid
graph BT
    subgraph Server host
        server[Rammingen server]
        db[("Database<br>(Postgres)")]
        storage[("Backup archive storage<br>(in the file system)")]
    end

    subgraph Client host
        client["Rammingen client<br>(encrypts/decrypts backup)"]
        directory[(Backed-up directory)]
    end

    server --> db
    server --> storage
    client --> server
    client --> directory
```
