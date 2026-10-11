FROM ubuntu:26.04
ENV TMPDIR=/root/tmp
RUN mkdir -p "$TMPDIR" && nice -n19 apt-get update && nice -n19 apt-get install -y --no-install-recommends python3 git && rm -rf /var/lib/apt/lists/*
