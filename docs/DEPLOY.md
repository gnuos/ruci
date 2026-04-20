# Ruci Deployment Guide

This guide covers various deployment methods for Ruci.

## Prerequisites

- Rust 1.90+ (for building from source)
- Docker & Docker Compose (for containerized deployment)
- SQLite, PostgreSQL, or MySQL (for database)

## Quick Start

### Using Docker Compose

```bash
# Start rucid
docker-compose -f contrib/docker-compose.yml up -d

# View logs
docker-compose -f contrib/docker-compose.yml logs -f
```

### From Source

```bash
# Build
make build

# Run
./bin/rucid --config contrib/ruci.yaml.example
```

---

## Deployment Methods

### 1. Docker Deployment

#### Building the Image

```bash
docker build -f contrib/docker/Dockerfile -t rucid:latest ..
```

#### Using Docker Compose

```yaml
# docker-compose.yml
services:
  rucid:
    image: rucid:latest
    ports:
      - "7741:7741"   # RPC
      - "8080:8080"   # Web UI
    volumes:
      - rucid-data:/var/lib/ruci
      - rucid-logs:/var/log/ruci
    environment:
      - RUCID_CONFIG=/etc/ruci/ruci.yaml
    restart: unless-stopped

volumes:
  rucid-data:
  rucid-logs:
```

#### Running Container

```bash
# Run with default config
docker run -d \
  --name rucid \
  -p 7741:7741 \
  -p 8080:8080 \
  -v /path/to/ruci.yaml:/etc/ruci/ruci.yaml \
  -v rucid-data:/var/lib/ruci \
  rucid:latest

# Run with environment variables
docker run -d \
  --name rucid \
  -p 7741:7741 \
  -p 8080:8080 \
  -e RUCID_CONFIG=/etc/ruci/ruci.yaml \
  rucid:latest
```

#### Container Health Check

```bash
curl http://localhost:8080/health
```

---

### 2. Systemd Deployment

#### Installation

```bash
# Build the binary
make build

# Run install script (creates user, directories, installs service)
sudo ./contrib/install.sh
```

Or manual installation:

```bash
# Create user
sudo useradd -r -m -s /bin/false rucid

# Create directories
sudo mkdir -p /var/lib/ruci/{jobs,run,archive,db}
sudo mkdir -p /var/run/ruci
sudo mkdir -p /var/log/ruci

# Set ownership
sudo chown -R rucid:rucid /var/lib/ruci /var/run/ruci /var/log/ruci

# Install binary
sudo cp bin/rucid /usr/local/bin/rucid
sudo cp contrib/rucid.service /etc/systemd/system/rucid.service

# Reload systemd and start
sudo systemctl daemon-reload
sudo systemctl enable rucid
sudo systemctl start rucid
```

#### Service Management

```bash
# Start the service
sudo systemctl start rucid

# Stop the service
sudo systemctl stop rucid

# Restart the service
sudo systemctl restart rucid

# Check status
sudo systemctl status rucid

# View logs
journalctl -u rucid -f
```

#### Configuration

Edit `/etc/ruci/ruci.yaml`:

```bash
sudo cp contrib/ruci.yaml.example /etc/ruci/ruci.yaml
sudo nano /etc/ruci/ruci.yaml
```

Key settings:

```yaml
server:
  host: "0.0.0.0"      # Bind to all interfaces for network access
  port: 7741
  web_host: "0.0.0.0"  # Bind to all interfaces
  web_port: 8080

database:
  url: "sqlite:///var/lib/ruci/db/ruci.db"

web:
  enabled: true
  admin_username: "admin"
  admin_password: "your-secure-password"  # Change this!
```

---

### 3. Manual Deployment

#### Directory Structure

```
/var/lib/ruci/
├── jobs/              # Job definition files
├── run/               # Build working directories
├── archive/           # Archived artifacts
├── db/                # SQLite database
└── logs/              # Log files
```

#### Steps

```bash
# 1. Build
make build

# 2. Create user and directories
sudo useradd -r -m -s /bin/false rucid
sudo mkdir -p /var/lib/ruci/{jobs,run,archive,db}
sudo mkdir -p /var/log/ruci
sudo mkdir -p /var/run/ruci

# 3. Set ownership
sudo chown -R rucid:rucid /var/lib/ruci /var/log/ruci /var/run/ruci

# 4. Install binary
sudo cp bin/rucid /usr/local/bin/rucid

# 5. Create config
sudo mkdir -p /etc/ruci
sudo cp contrib/ruci.yaml.example /etc/ruci/ruci.yaml

# 6. Run
sudo -u rucid /usr/local/bin/rucid --config /etc/ruci/ruci.yaml
```

---

## Configuration Reference

### Server Configuration

```yaml
server:
  host: "0.0.0.0"       # RPC bind address
  port: 7741            # RPC port
  web_host: "0.0.0.0"   # Web UI bind address
  web_port: 8080        # Web UI port
  rpc_mode: "tcp"       # "tcp" or "unix"
```

### Database Configuration

```yaml
# SQLite
database:
  url: "sqlite:///var/lib/ruci/db/ruci.db"

# PostgreSQL
database:
  url: "postgresql://ruci:password@localhost:5432/ruci"

# MySQL
database:
  url: "mysql://ruci:password@localhost:3306/ruci"
```

### Storage Configuration

```yaml
# Local storage
storage:
  type: "local"

# S3/Rustfs storage
storage:
  type: "rustfs"
  endpoint: "http://localhost:9000"
  bucket: "ruci-artifacts"
  access_key: "${AWS_ACCESS_KEY_ID}"
  secret_key: "${AWS_SECRET_ACCESS_KEY}"
  region: "us-east-1"
```

### Context Configuration (Resource Limits)

```yaml
contexts:
  default:
    max_parallel: 4     # Max concurrent jobs
    timeout: 3600       # Job timeout (seconds)
    work_dir: "/tmp"    # Working directory

  high-memory:
    max_parallel: 2
    timeout: 7200
    work_dir: "/tmp/ruci"
```

### Logging Configuration

```yaml
logging:
  level: "info"         # trace, debug, info, warn, error
  format: "json"        # "json" or "pretty"
  file:
    dir: "/var/log/ruci"
    max_size_mb: 100
    max_files: 7
```

### Web UI Authentication

```yaml
web:
  enabled: true
  admin_username: "admin"
  admin_password: "admin"  # Change in production!
```

---

## Reverse Proxy Configuration

### Nginx

```nginx
server {
    listen 80;
    server_name ruci.example.com;

    # Web UI
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection 'upgrade';
        proxy_set_header Host $host;
        proxy_cache_bypass $http_upgrade;
    }

    # RPC (if using TCP mode)
    location /rpc {
        proxy_pass http://127.0.0.1:7741;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
    }
}
```

### Caddy

```caddy
ruci.example.com {
    reverse_proxy localhost:8080
}
```

---

## Security Considerations

1. **Change default credentials** - Update `web.admin_password` immediately
2. **Use HTTPS** - Deploy behind a reverse proxy with TLS
3. **Network access** - Restrict access to port 8080 (Web UI) and 7741 (RPC)
4. **File permissions** - Ensure directories are owned by `rucid` user
5. **Webhook secrets** - Use strong secrets for webhook authentication
6. **SSH keys** - Store VCS SSH keys securely with proper permissions

---

## Troubleshooting

### Check logs

```bash
# Systemd
journalctl -u rucid -n 100

# Docker
docker logs rucid

# Manual
tail -f /var/log/ruci/rucid.log
```

### Check health

```bash
curl http://localhost:8080/health
```

### Check metrics

```bash
curl http://localhost:8080/metrics
```

### Common Issues

**Port already in use:**
```bash
# Check what's using the port
ss -tlnp | grep 8080

# Change port in config
```

**Permission denied:**
```bash
# Fix ownership
sudo chown -R rucid:rucid /var/lib/ruci /var/log/ruci
```

**Database locked:**
```bash
# Ensure only one instance is running
sudo systemctl stop rucid
sudo rm /var/lib/ruci/db/ruci.db
sudo systemctl start rucid
```
