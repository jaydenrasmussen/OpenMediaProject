# Build stage for Go Backend
FROM golang:1.21-alpine AS backend-builder
WORKDIR /app/backend

# Copy go.mod and go.sum and download dependencies
COPY backend/go.mod backend/go.sum ./
RUN go mod download

# Copy backend source code
COPY backend/ ./

# Build backend binary
RUN CGO_ENABLED=0 GOOS=linux go build -o server .

# Build stage for SvelteKit Frontend
FROM node:24-slim AS frontend-builder
WORKDIR /app/frontend

# Copy package.json and install dependencies
COPY frontend/package*.json ./
RUN npm install

# Copy frontend source code
COPY frontend/ ./

# Build static frontend
ENV VITE_API_URL=""
RUN npm run build

# Final stage
FROM alpine:3.19

# Install necessary packages
RUN apk --no-cache add ca-certificates tzdata

# Set up work directory
WORKDIR /app

# Copy built backend binary from backend-builder stage
COPY --from=backend-builder /app/backend/server /app/server

# Copy built SvelteKit static app from frontend-builder stage
COPY --from=frontend-builder /app/frontend/build /app/public

# Environment variables
ENV SERVER_ADDRESS=:8080
ENV CORS_ALLOW_ORIGINS=*
ENV NODE_ENV=production
ENV DB_PATH=/app/data/nutsdb
ENV VITE_API_URL=""
ENV TVDB_API_KEY=""
ENV TMDB_API_KEY=""
ENV RUTORRENT_URL=""
ENV RUTORRENT_USERNAME=""
ENV RUTORRENT_PASSWORD=""

# Expose port
EXPOSE 8080

# Create data directory for NutsDB and set permissions
RUN mkdir -p /app/data/nutsdb
RUN adduser -D -u 1000 appuser
RUN chown -R appuser:appuser /app/data
USER appuser

# Run the application
CMD ["/app/server"]
