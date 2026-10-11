FROM ubuntu:26.04
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc libc6-dev gcc-mingw-w64-x86-64-posix ca-certificates && rm -rf /var/lib/apt/lists/*
