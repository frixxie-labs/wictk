# WICTK (Weather Is Cool To Know) - Design Document

## Overview

WICTK is a weather and environmental event aggregation system designed to collect, process, and distribute current information from multiple providers. The system provides weather nowcasts, lightning strikes, weather alerts, and recent earthquakes through a REST API, with additional components for data collection and notification delivery.

### Core Features

- **Multi-source Weather Data**: Aggregates data from MET Norway and OpenWeatherMap
- **Lightning Detection**: Real-time lightning strike monitoring and reporting
- **Earthquake Monitoring**: Recent USGS earthquakes with magnitude and radius filtering
- **Weather Alerts**: Automated alert monitoring and notification system
- **Location Services**: Geocoding and coordinate-based weather queries
- **High Performance**: In-memory caching and async processing
- **Observability**: Comprehensive metrics and structured logging
- **Data Export**: Integration with external monitoring systems (HEMRS)

## Architecture

WICTK follows a microservices architecture with a shared core library, implemented as a Rust workspace with four main components:

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Client Logger │    │     Notifier    │    │     Backend     │
│                 │    │                 │    │                 │
│ • Data Collection│    │ • Alert Monitor │    │ • REST API      │
│ • HEMRS Export   │    │ • NTFY Notifier │    │ • Caching       │
│ • Sensor Setup   │    │ • Polling       │    │ • Metrics       │
└─────────────────┘    └─────────────────┘    └─────────────────┘
         │                       │                       │
         └───────────────────────┼───────────────────────┘
                                 │
                    ┌─────────────────┐
                    │   wictk_core    │
                    │                 │
                    │ • Data Models   │
                    │ • Business Logic│
                    │ • API Clients   │
                    └─────────────────┘
```

### Component Responsibilities

#### Backend Service
The main API server built with Axum, providing:
- RESTful endpoints for weather data queries
- Multi-level caching (Moka) for performance
- Prometheus metrics integration
- Request profiling and logging
- CORS and security middleware

#### Core Library (wictk_core)
Shared business logic containing:
- Data structures for weather entities (Nowcast, Lightning, Alert)
- API client implementations for external services
- Location and coordinate handling
- Serialization/deserialization logic

#### Client Logger
Data collection and export service featuring:
- Automated weather data fetching from WICTK API
- Integration with HEMRS monitoring system
- Device and sensor management
- Parallel processing with Rayon
- Temperature ratio calculations

#### Notifier
Alert monitoring and notification service:
- Continuous polling of weather alerts
- NTFY notification delivery
- Alert deduplication and filtering
- Configurable notification channels

## Data Flow

### Weather Data Pipeline

```
MET Norway API ──┐
                 ├──► Backend Service ───► Client Logger ───► HEMRS
OpenWeatherMap ──┘          │
                            │
                            ▼
                       Notifier Service ───► NTFY
```

1. **Data Ingestion**: Backend fetches data from MET Norway and OpenWeatherMap APIs
2. **Caching**: Responses cached for 5 minutes to reduce external API load
3. **API Serving**: REST endpoints serve cached and fresh data
4. **Data Export**: Client Logger consumes API data and exports to HEMRS
5. **Alert Monitoring**: Notifier polls alerts endpoint and sends notifications

### Lightning Data Flow

```
YR.no Lightning API ───► Backend Service ───► Client Logger ───► HEMRS
       (24h data)               │                     │
                                │                     │
                                ▼                     ▼
                           REST API            Lightning Device
```

Lightning data follows a similar pattern but with specialized filtering for recent strikes (within 10 minutes).

## API Design

### REST Endpoints

#### Weather Data
- `GET /api/nowcasts?location={city}` - Combined MET + OpenWeather nowcasts
- `GET /api/met/nowcasts?location={city}` - MET Norway data only
- `GET /api/owm/nowcasts?location={city}` - OpenWeatherMap data only

#### Lightning Data
- `GET /api/recent_lightning` - All recent lightning strikes (24h)
- `GET /api/recent_lightning?location={city}&radius_km={km}` - Filtered by location

#### Earthquake Data
- `GET /api/earthquakes` - Recent earthquakes from the USGS daily feed
- `GET /api/earthquakes?min_magnitude={magnitude}&limit={count}` - Filtered by magnitude
- `GET /api/earthquakes?location={city}&radius_km={km}` - Filtered around a city
- `GET /api/earthquakes?lat={latitude}&lon={longitude}&radius_km={km}` - Filtered around coordinates

#### Alerts & Location
- `GET /api/alerts` - Current weather alerts
- `GET /api/geocoding?location={query}` - Location search and coordinates

#### System
- `GET /status/ping` - Health check
- `GET /status/health` - Detailed health status
- `GET /metrics` - Prometheus metrics

### Request/Response Format

#### Location Query Parameters
```json
{
  "location": "Oslo",           // City name
  "lat": "59.91273",           // Latitude (alternative to location)
  "lon": "10.74609",           // Longitude (alternative to location)
  "radius_km": 50              // Search radius (default: 50km)
}
```

#### Nowcast Response
```json
[
  {
    "met": {
      "time": "2025-01-20T12:00:00Z",
      "location": {"lon": 10.4034, "lat": 63.4308},
      "description": "Clear sky",
      "air_temperature": 15.2,
      "relative_humidity": 65.0,
      "precipitation_rate": 0.0,
      "wind_speed": 3.5,
      "wind_from_direction": 180.0
    }
  },
  {
    "open_weather": {
      "dt": "2025-01-20T12:00:00Z",
      "name": "Trondheim",
      "main": "Clear",
      "desc": "clear sky",
      "temp": 15.2,
      "humidity": 65,
      "wind_speed": 3.5,
      "wind_deg": 180
    }
  }
]
```

#### Lightning Response
```json
[
  {
    "time": "2025-01-20T11:45:00Z",
    "location": {"x": 10.4034, "y": 63.4308},
    "magic_value": 42
  }
]
```

#### Earthquake Response
```json
[
  {
    "id": "us7000example",
    "magnitude": 4.7,
    "place": "12 km NW of Example",
    "time": "2026-08-11T06:38:56Z",
    "updated": "2026-08-11T06:42:10Z",
    "coordinates": {"lon": 10.2, "lat": 59.1},
    "depth_km": 8.3,
    "significance": 340,
    "alert_level": "green",
    "status": "reviewed",
    "tsunami": false,
    "details_url": "https://earthquake.usgs.gov/earthquakes/eventpage/us7000example"
  }
]
```

## Data Sources

### MET Norway (Meteorologisk Institutt)
- **API**: https://api.met.no
- **Data**: Official Norwegian weather forecasts and observations
- **License**: Requires attribution, rate limited
- **Usage**: Nowcasts, weather alerts

### OpenWeatherMap
- **API**: https://openweathermap.org/api
- **Data**: Global weather data with Norwegian coverage
- **License**: Commercial API key required
- **Usage**: Supplementary nowcasts, geocoding

### Lightning Data (YR.no)
- **API**: https://www.yr.no/api/v0/lightning-events
- **Data**: Real-time lightning strike data for Norway
- **Format**: Historical data in 24-hour windows
- **Usage**: Lightning monitoring and alerts

### Earthquake Data (USGS)
- **API**: https://earthquake.usgs.gov/earthquakes/feed/v1.0/geojson.php
- **Data**: Global earthquakes from the versioned `all_day` GeoJSON feed
- **Update cadence**: Every minute
- **Usage**: Recent earthquake monitoring with local WICTK filtering

## Caching Strategy

### Cache Configuration
- **Location Cache**: 20 entries, 5-minute TTL
- **Nowcast Cache**: 20 entries, 5-minute TTL
- **Alert Cache**: 1 entry, 5-minute TTL
- **Lightning Cache**: 1 entry, 5-minute TTL
- **Earthquake Cache**: 1 entry, 1-minute TTL

### Cache Keys
- Location-based: `{provider}_{location}_{radius}`
- Time-based: Automatic expiration
- Size-limited: LRU eviction when capacity reached

## Performance Characteristics

### Throughput
- **API Response Time**: <100ms (cached), <2s (fresh)
- **Concurrent Requests**: 1000+ concurrent connections
- **Memory Usage**: ~50MB base + cache growth

### Scalability
- Horizontal scaling via load balancer
- Stateless design (cache replication needed for HA)
- External service rate limits as bottleneck

## Deployment

### Containerized Deployment
```yaml
# Kubernetes deployment with:
# - Backend service (port 3000)
# - Notifier (background job)
# - Client Logger (cron job)
# - Redis for cache replication (future)
```

### Environment Configuration
```bash
# Required
OPENWEATHERMAPAPIKEY=your_api_key

# Optional
HOST=0.0.0.0:3000
LOG_LEVEL=info
USGS_EARTHQUAKE_FEED_URL=https://earthquake.usgs.gov/earthquakes/feed/v1.0/summary/all_day.geojson
```

### Health Checks
- `/status/ping` - Basic connectivity
- `/status/health` - External service dependencies
- `/metrics` - Performance metrics

## Development

### Build System
- **Rust Edition**: 2021
- **Workspace**: Multi-crate with shared dependencies
- **Build**: `cargo build` / `make build`
- **Test**: `cargo test` / `make test`
- **Lint**: `cargo check` / `make check`

### Code Organization
```
wictk/
├── backend/          # API server
├── wictk_core/       # Shared library
├── client_logger/    # Data exporter
├── notifier/         # Alert system
```

### Testing Strategy
- **Unit Tests**: Core business logic
- **Integration Tests**: API endpoints
- **Mock Tests**: External API dependencies
- **Load Tests**: Performance validation (Locust)

### Observability
- **Logging**: Newline-delimited, flattened JSON logs with tracing, compatible with VictoriaLogs
- **Metrics**: Prometheus histograms for request latency
- **Profiling**: Request timing middleware
- **Health**: Dependency health checks

Send container stdout to VictoriaLogs' `/insert/jsonline` endpoint with
`_msg_field=message&_time_field=timestamp`. Use stable deployment metadata such
as the application and namespace as stream fields in the log collector.

## Security Considerations

### API Keys
- OpenWeatherMap key stored as `Secret<String>`
- Environment variable injection
- No key exposure in logs

### Data Validation
- Input sanitization on all endpoints
- Coordinate bounds checking
- Rate limiting via external services

### Network Security
- HTTPS-only external API calls
- Input validation and SQL injection prevention
- CORS configuration for web clients

## Future Enhancements

### Planned Features
- **Cache Replication**: Redis for multi-instance deployment
- **Alert Filtering**: User-configurable alert preferences
- **Historical Data**: Time-series weather data storage
- **Web Dashboard**: Real-time weather visualization
- **Mobile App**: Native weather application

### Technical Debt
- Error handling standardization
- Database integration for persistent storage
- API versioning strategy

---

## Quick Start

```bash
# Build all components
cargo build --release

# Run backend (requires OpenWeatherMap API key)
OPENWEATHERMAPAPIKEY=xxx cargo run --bin backend

# Test API
curl "http://localhost:3000/api/nowcasts?location=Oslo"
curl "http://localhost:3000/api/earthquakes?min_magnitude=4.5&limit=20"
```
