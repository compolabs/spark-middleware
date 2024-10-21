# Use an official Rust image as the base image
FROM rust:bookworm as builder

# Set the working directory inside the container
WORKDIR /usr/src/spark-middleware

# Copy the Cargo.toml and lock files to get dependencies
COPY Cargo.toml Cargo.lock ./

# Copy the source code into the container
COPY . .

# Install dependencies and compile the application in release mode
RUN apt-get update && \
    apt-get install -y pkg-config && \
    apt-get install -y openssl && \
    apt-get install -y libssl-dev && \
    rustup target add wasm32-unknown-unknown && \
    cargo build --release

# Use a smaller base image for the final container
FROM debian:bookworm-slim


# Set working directory
WORKDIR /usr/local/bin

# Copy the built binary from the builder stage
COPY --from=builder /usr/src/spark-middleware/target/release/spark-middleware .

# Expose the necessary port for the application
EXPOSE 9001
EXPOSE 9090

# Set the default command to run the application
CMD ["./spark-middleware"]
