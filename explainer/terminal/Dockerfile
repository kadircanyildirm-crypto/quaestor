FROM rust:1
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
      curl clang pkg-config libssl-dev protobuf-compiler \
      ffmpeg chromium fonts-jetbrains-mono fonts-dejavu-core \
 && rm -rf /var/lib/apt/lists/*
RUN curl -sSfL -o /usr/local/bin/ttyd https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.x86_64 \
 && chmod +x /usr/local/bin/ttyd \
 && curl -sSfL -o /tmp/vhs.deb https://github.com/charmbracelet/vhs/releases/download/v0.12.1/vhs_0.12.1_amd64.deb \
 && dpkg -i /tmp/vhs.deb && rm /tmp/vhs.deb
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends libprotobuf-dev \
 && rm -rf /var/lib/apt/lists/*
ENV VHS_NO_SANDBOX=true
