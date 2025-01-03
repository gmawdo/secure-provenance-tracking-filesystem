# Use Debian as the base image
FROM debian:latest
#FROM ubuntu:20.04

# Avoid prompts from apt
ENV DEBIAN_FRONTEND=noninteractive

# Install required packages and Redis
RUN apt-get update && \
    apt-get install -y nfs-common curl build-essential pkg-config libssl-dev redis-server redis-tools npm && \
    rm -rf /var/lib/apt/lists/*

RUN npm install -g wscat

# Install Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y

# Set the PATH environment variable to include the Cargo bin directory
ENV PATH="/root/.cargo/bin:${PATH}"

# Create the necessary directories
#RUN mkdir -p /mnt/nfs /mount_point /app/Redis_database/redis-6380 /app/Redis_database/redis-6381 /app/Redis_database/redis-6382

# Copy the source code into the image
COPY . /app
WORKDIR /app

# Build the Rust project
RUN cargo build --bin graymamba --features="traceability" --release

# Copy the built binary to a location in the PATH
RUN cp /app/target/release/graymamba /usr/local/bin/graymamba

# Make the graymamba executable
RUN chmod +x /usr/local/bin/graymamba

# Expose the necessary ports
EXPOSE 2049 9944 6380 6381 6382

# Copy and make the entrypoint script executable
COPY entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Command to run when the container starts
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
