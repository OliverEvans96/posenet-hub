FROM gitlab-registry.nautilus.optiputer.net/librareome/posenet/posenet-hub/rust-deps AS build

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
# Build the project
RUN cargo build --release
# Make sure everything is working
RUN cargo test --release

# The final image only needs the compiled binaries
FROM ubuntu
RUN apt-get update && apt-get install -y netbase
COPY --from=build /usr/local/src/posenet-hub/target/release/hub-server /usr/local/bin/
COPY --from=build /usr/local/src/posenet-hub/target/release/grpc-client /usr/local/bin/
COPY --from=build /usr/local/src/posenet-hub/target/release/vrpn-client /usr/local/bin/
COPY --from=build /etc/version-info /etc/

# Final config
RUN useradd -m posenet
USER posenet
WORKDIR /home/posenet
CMD hub-server
# gRPC
EXPOSE 50051
# VRPN
EXPOSE 3038
