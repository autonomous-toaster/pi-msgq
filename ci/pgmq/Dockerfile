# A PostgreSQL with pgmq installed, for the shared engine's tests.
#
# pgmq ships its schema as plain SQL, so no compilation and no superuser packaging are needed: the
# file lands in the entrypoint's init directory and the default database has pgmq on first start.
# The version is pinned in the build argument, so a rebuild is reproducible and CI depends on the
# upstream project rather than on a third-party image publisher.
FROM docker.io/library/postgres:18

ARG PGMQ_VERSION=v1.13.1

ADD --chmod=0644 \
    https://raw.githubusercontent.com/pgmq/pgmq/${PGMQ_VERSION}/pgmq-extension/sql/pgmq.sql \
    /docker-entrypoint-initdb.d/00-pgmq.sql
