#!/bin/bash
# Will only run inside the DB container if the data directory is empty.

find /migrations/ -name "*.sql" | \
    sort | \
    xargs -n 1 psql -v ON_ERROR_STOP=1 --dbname "$POSTGRES_DB" --username "$POSTGRES_USER" -f
