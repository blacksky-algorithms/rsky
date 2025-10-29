#!/bin/bash

# Local Development Helper Script for rsky
set -e

COMPOSE_FILE="docker-compose.local.yml"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

function print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

function print_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

function print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

function show_help() {
    cat << EOF
rsky Local Development Helper

Usage: $0 <command>

Commands:
    build           Build all Docker images
    build-ingester  Build only ingester image
    build-indexer   Build only indexer image
    build-backfiller Build only backfiller image

    start           Start minimal setup (redis + ingester)
    start-full      Start full pipeline (redis + postgres + all services)

    stop            Stop all services
    restart         Restart all services
    clean           Stop and remove all data

    logs            Show logs for all services
    logs-ingester   Show logs for ingester only
    logs-indexer    Show logs for indexer only
    logs-backfiller Show logs for backfiller only

    redis           Open Redis CLI
    redis-stats     Show Redis stream stats
    redis-clear     Clear all Redis data

    status          Show container status
    help            Show this help message

Examples:
    $0 build            # Build all images
    $0 start            # Start ingester + Redis
    $0 logs-ingester    # Watch ingester logs
    $0 redis-stats      # Check Redis streams
EOF
}

function build_all() {
    print_info "Building all Docker images (this may take a while)..."

    print_info "Building rsky-ingester..."
    docker build --no-cache -t rsky-ingester:latest -f rsky-ingester/Dockerfile .

    print_info "Building rsky-indexer..."
    docker build --no-cache -t rsky-indexer:latest -f rsky-indexer/Dockerfile .

    print_info "Building rsky-backfiller..."
    docker build --no-cache -t rsky-backfiller:latest -f rsky-backfiller/Dockerfile .

    print_info "✓ All images built successfully"
}

function build_ingester() {
    print_info "Building rsky-ingester..."
    docker build --no-cache -t rsky-ingester:latest -f rsky-ingester/Dockerfile .
    print_info "✓ Ingester built successfully"
}

function build_indexer() {
    print_info "Building rsky-indexer..."
    docker build --no-cache -t rsky-indexer:latest -f rsky-indexer/Dockerfile .
    print_info "✓ Indexer built successfully"
}

function build_backfiller() {
    print_info "Building rsky-backfiller..."
    docker build --no-cache -t rsky-backfiller:latest -f rsky-backfiller/Dockerfile .
    print_info "✓ Backfiller built successfully"
}

function start_minimal() {
    print_info "Starting minimal setup (Redis + Ingester)..."
    docker compose -f "$COMPOSE_FILE" up -d redis ingester
    print_info "✓ Services started"
    print_info "Watch logs with: $0 logs-ingester"
    print_info "Check Redis with: $0 redis-stats"
}

function start_full() {
    print_info "Starting full pipeline (Redis + Postgres + All services)..."
    docker compose -f "$COMPOSE_FILE" --profile full up -d
    print_info "✓ Services started"
    print_info "Watch logs with: $0 logs"
    print_warn "Database schema needs to be initialized manually!"
}

function stop_services() {
    print_info "Stopping all services..."
    docker compose -f "$COMPOSE_FILE" down
    print_info "✓ Services stopped"
}

function restart_services() {
    print_info "Restarting all services..."
    docker compose -f "$COMPOSE_FILE" restart
    print_info "✓ Services restarted"
}

function clean_all() {
    print_warn "This will remove all containers and volumes (data will be lost)"
    read -p "Are you sure? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        print_info "Cleaning up..."
        docker compose -f "$COMPOSE_FILE" down -v
        print_info "✓ All data cleaned"
    else
        print_info "Cancelled"
    fi
}

function show_logs() {
    docker compose -f "$COMPOSE_FILE" logs -f "$@"
}

function redis_cli() {
    print_info "Opening Redis CLI (type 'exit' to quit)..."
    docker exec -it rsky-redis-local redis-cli
}

function redis_stats() {
    print_info "Redis Stream Statistics"
    echo "------------------------"
    docker exec rsky-redis-local redis-cli << EOF
ECHO "Firehose Live Stream:"
XLEN firehose_live
ECHO ""
ECHO "Repo Backfill Stream:"
XLEN repo_backfill
ECHO ""
ECHO "Label Live Stream:"
XLEN label_live
ECHO ""
ECHO "Cursors:"
KEYS *cursor*
EOF
}

function redis_clear() {
    print_warn "This will clear ALL Redis data"
    read -p "Are you sure? (y/N) " -n 1 -r
    echo
    if [[ $REPLY =~ ^[Yy]$ ]]; then
        print_info "Clearing Redis..."
        docker exec rsky-redis-local redis-cli FLUSHALL
        print_info "✓ Redis cleared"
    else
        print_info "Cancelled"
    fi
}

function show_status() {
    print_info "Container Status:"
    docker compose -f "$COMPOSE_FILE" ps
}

# Main command dispatcher
case "$1" in
    build)
        build_all
        ;;
    build-ingester)
        build_ingester
        ;;
    build-indexer)
        build_indexer
        ;;
    build-backfiller)
        build_backfiller
        ;;
    start)
        start_minimal
        ;;
    start-full)
        start_full
        ;;
    stop)
        stop_services
        ;;
    restart)
        restart_services
        ;;
    clean)
        clean_all
        ;;
    logs)
        show_logs
        ;;
    logs-ingester)
        show_logs ingester
        ;;
    logs-indexer)
        show_logs indexer
        ;;
    logs-backfiller)
        show_logs backfiller
        ;;
    redis)
        redis_cli
        ;;
    redis-stats)
        redis_stats
        ;;
    redis-clear)
        redis_clear
        ;;
    status)
        show_status
        ;;
    help|--help|-h|"")
        show_help
        ;;
    *)
        print_error "Unknown command: $1"
        echo ""
        show_help
        exit 1
        ;;
esac
