FROM gitlab-registry.nautilus.optiputer.net/librareome/posenet/posenet-hub/cpp-deps

# Install PoseNet Hub (this repo)
WORKDIR /usr/local/src/

# Create a fake package with real dependencies
# to build the dependencies first 
# to maximize Docker caching
RUN USER=root cargo new --lib posenet-hub
WORKDIR /usr/local/src/posenet-hub
RUN rustup component add rustfmt
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release