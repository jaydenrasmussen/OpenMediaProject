#!/bin/sh
apt-get update -qq >/dev/null
DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends inn2 netcat-openbsd >/dev/null 2>&1
cp /inn/readers.conf /etc/news/readers.conf
exec /inn/start.sh
