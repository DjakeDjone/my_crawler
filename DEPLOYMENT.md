# Deployment

The production stack runs with Docker Compose on the Hetzner server available
locally as `ssh hetzner`. GitHub Actions tests, builds, publishes, and deploys
every push to `master`.

## Production layout

| Item | Value |
| --- | --- |
| Repository | `DjakeDjone/my_crawler` |
| Workflow | `.github/workflows/deploy.yml` |
| Server directory | `/opt/my_crawler` |
| Compose project | `my_crawler` |
| API | Host port `8000` |
| Spider | Host port `8001` |
| Qdrant | Internal ports `6333` and `6334` |
| TEI | Internal port `80` |
| API image | `ghcr.io/djakedjone/my_crawler-api:latest` |
| Spider image | `ghcr.io/djakedjone/my_crawler-spider:latest` |

Qdrant and TEI are internal-only. The API and spider are published on all host
interfaces. Persistent data lives in the named `qdrant-data` and `model-cache`
volumes. Recreating containers does not delete these volumes.

The 4 GB server has swap enabled. TEI is limited to 1.5 GB and uses reduced
batch concurrency; these values are intentional for this host.

## Automatic deployment

The `Deploy` workflow runs on:

- every push to `master`;
- a manual `workflow_dispatch`.

It performs:

1. `cargo test --workspace`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `docker compose config`
4. builds the API and spider images;
5. pushes both images to GHCR;
6. copies the repository to `/opt/my_crawler` without replacing `.env`;
7. pulls images and runs `docker compose up -d --wait --remove-orphans`;
8. removes unused images.

Only one production deployment runs at a time.

### GitHub configuration

The repository needs these Actions secrets:

| Secret | Purpose |
| --- | --- |
| `HETZNER_HOST` | Server hostname or IP |
| `HETZNER_USER` | SSH user |
| `HETZNER_SSH_KEY` | Private deployment key |
| `HETZNER_KNOWN_HOSTS` | Pinned SSH host-key entry |

The deploy job uses the GitHub `production` environment and the built-in
`GITHUB_TOKEN` to publish images. Its workflow permissions require
`contents: read` and `packages: write`.

The matching public deployment key must be present in the server user's
`~/.ssh/authorized_keys`.

## Environment file

`/opt/my_crawler/.env` is managed directly on the server. It is deliberately
excluded from rsync and must exist before deployment.

```dotenv
API_PORT=8000
SPIDER_PORT=8001
CRAWLER_PRODUCT_TOKEN=MyCrawler
CRAWLER_USER_AGENT=MyCrawler/1.0 (+https://github.com/DjakeDjone/my_crawler)
ALLOWED_ORIGINS=http://localhost:3000
```

Edit it without printing secrets:

```bash
ssh hetzner
cd /opt/my_crawler
nano .env
chmod 600 .env
docker compose up -d --wait
```

## Deploy or redeploy

Normal deployment:

```bash
git push origin master
```

Start the workflow manually:

```bash
gh workflow run deploy.yml --ref master
gh run watch "$(gh run list --workflow deploy.yml --limit 1 --json databaseId --jq '.[0].databaseId')" --exit-status
```

Redeploy the current published images directly on the server:

```bash
ssh hetzner
cd /opt/my_crawler
docker compose pull
docker compose up -d --wait --remove-orphans
```

Restart without pulling:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose restart'
```

## Status and health

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose ps'
ssh hetzner 'curl -fsS http://127.0.0.1:8000/health'
ssh hetzner 'curl -fsS http://127.0.0.1:8001/health'
```

Expected health responses:

```json
{"message":"API is running","status":"ok"}
{"message":"Crawler API is running","status":"ok"}
```

Inspect resource usage:

```bash
ssh hetzner 'docker stats --no-stream'
ssh hetzner 'free -h && df -h'
```

## Logs

Follow all services:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose logs -f'
```

Follow one or more services:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose logs -f api'
ssh hetzner 'cd /opt/my_crawler && docker compose logs -f spider'
ssh hetzner 'cd /opt/my_crawler && docker compose logs -f tei qdrant'
```

Show recent logs without following:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose logs --tail=200 api spider'
```

Show logs since a time:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose logs --since=30m'
```

View GitHub Actions logs:

```bash
gh run list --workflow deploy.yml
gh run view --log-failed
```

## Common failures

### API or spider is not starting

Both wait for healthy Qdrant and TEI:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose ps -a'
ssh hetzner 'cd /opt/my_crawler && docker compose logs --tail=200 tei qdrant api spider'
```

### Exit code 137

The container was usually killed for exceeding its memory limit:

```bash
ssh hetzner 'docker inspect my_crawler-tei-1 --format "OOMKilled={{.State.OOMKilled}} Exit={{.State.ExitCode}}"'
ssh hetzner 'free -h'
```

Do not raise all memory limits together; their total must fit the 4 GB host.

### Deployment rejects `.env`

Create `/opt/my_crawler/.env`, add the required crawler identity, and set mode
`600`. The workflow intentionally refuses to deploy without it.

### Image pull fails

Confirm that GHCR packages are readable by the server and that the workflow's
`packages: write` permission is still present. Then rerun the workflow.

## Data and destructive operations

Stop containers while preserving data:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose down'
```

Do **not** use `docker compose down -v` unless Qdrant data and the model cache
should be deleted. Existing Weaviate data is not part of this deployment.

Back up Qdrant data:

```bash
ssh hetzner 'docker run --rm -v my_crawler_qdrant-data:/data -v /root:/backup alpine tar czf /backup/qdrant-data.tgz -C /data .'
```

Restore only while the stack is stopped:

```bash
ssh hetzner 'cd /opt/my_crawler && docker compose down'
ssh hetzner 'docker run --rm -v my_crawler_qdrant-data:/data -v /root:/backup alpine sh -c "find /data -mindepth 1 -delete && tar xzf /backup/qdrant-data.tgz -C /data"'
ssh hetzner 'cd /opt/my_crawler && docker compose up -d --wait'
```

## Updating the stack

Change pinned service versions or application configuration in `compose.yml`,
validate locally, then push:

```bash
CRAWLER_PRODUCT_TOKEN=CI CRAWLER_USER_AGENT=CI/1.0 docker compose config
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git push origin master
```

Application images currently use the `latest` tag. To roll back application
code, revert the bad commit on `master` and let the workflow rebuild and
redeploy it.
