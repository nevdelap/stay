FROM docker.io/library/debian:bookworm-slim

RUN <<'EOF'
set -eux
apt-get update
apt-get install --no-install-recommends --yes \
	build-essential \
	ca-certificates \
	zlib1g-dev
rm -rf /var/lib/apt/lists/*
EOF
