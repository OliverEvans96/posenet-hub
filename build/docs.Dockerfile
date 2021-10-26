# Inspired by https://gitlab.com/gastove/fibonacci-example/-/blob/master/Dockerfile
FROM gitlab-registry.nrp-nautilus.io/librareome/posenet/posenet-hub/rust-deps AS build

ARG SRC_COMMIT
ENV SRC_COMMIT=$SRC_COMMIT
RUN echo "CPP_DEPS_COMMIT=$CPP_DEPS_COMMIT"
RUN echo "RUST_DEPS_COMMIT=$RUST_DEPS_COMMIT"
RUN echo "SRC_COMMIT=$SRC_COMMIT"

# Compile PoseNet Hub

# Delete the phony lib
RUN rm src/lib.rs

# Copy the project files
COPY build.rs .
COPY include/ ./include
COPY proto/ ./proto
COPY src/ ./src
COPY tests/ ./tests

# Build the documentation
RUN cargo doc --no-deps --document-private-items

FROM nginx:1.17.10

COPY --from=build /app/target/doc /var/www

ADD build/nginx.conf /etc/nginx/conf.d/default.conf

