#!/bin/bash

# Start development environment for OpenMedia Project
# This script starts all required services using docker-compose

set -e

# Change to the project root directory
cd "$(dirname "$0")/.." || exit

# Check if docker-compose is installed
if ! command -v docker-compose &> /dev/null; then
    echo "Error: docker-compose is not installed or not in the PATH"
    exit 1
fi

# Print a header
echo "=================================="
echo "  OpenMedia Project - Dev Setup   "
echo "=================================="

# Check if .env file exists and source it
if [ -f .env ]; then
    echo "Loading environment variables from .env file"
    source .env
fi

# Clean up function for graceful shutdown
cleanup() {
    echo "Shutting down services..."
    docker-compose down
    echo "Dev environment stopped."
    exit 0
}

# Set trap for Ctrl+C and other termination signals
trap cleanup INT TERM

# Start services
echo "Starting development services..."
docker-compose up -d

# Display logs
echo "Services started. Showing logs:"
echo "Press Ctrl+C to stop"
docker-compose logs -f

# This will be reached when logs are exited but not with Ctrl+C
cleanup
