FROM docker.io/library/node:22-bookworm-slim@sha256:4d676821dff059fd00d277ee4261ef34ea712317fed0737c03941481b5760c96
USER root
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN npm install --global --omit=dev vibe-kanban@0.1.44
USER node
ENV HOME=/tmp/vibe-home HOST=0.0.0.0 PORT=3000 BROWSER=none NO_COLOR=1
ENTRYPOINT ["vibe-kanban"]
