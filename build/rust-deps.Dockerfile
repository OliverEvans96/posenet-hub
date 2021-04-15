FROM gitlab-registry.nautilus.optiputer.net/librareome/posenet/posenet-hub/cpp-deps

ARG RUST_DEPS_COMMIT
ENV RUST_DEPS_COMMIT=$RUST_DEPS_COMMIT
RUN echo "CPP_DEPS_COMMIT=$CPP_DEPS_COMMIT"
RUN echo "RUST_DEPS_COMMIT=$RUST_DEPS_COMMIT"

# Create a fake package with real dependencies
# to build the dependencies first 
# to maximize Docker caching
WORKDIR /usr/local/src/
RUN USER=root cargo new --lib posenet-hub
WORKDIR /usr/local/src/posenet-hub
RUN rustup component add rustfmt
COPY Cargo.toml Cargo.lock ./
RUN cargo build --release