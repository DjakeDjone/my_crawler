# Port Configuration

Only the Rust HTTP services are published by default:

| Service | Environment variable | Default |
| --- | --- | --- |
| Search API | `API_PORT` | `8000` |
| Crawler API | `SPIDER_PORT` | `8001` |

Qdrant (`6333`, `6334`) and TEI (`80`) remain internal to the Compose network.
For local non-Compose development, configure `QDRANT_URL` and `TEI_URL` as
shown in `.env.example`.
