FROM rust AS build

# Install build dependencies
RUN apt-get update && apt-get install -y libeigen3-dev cmake

# Install OpenMVG (C++ multi-view geometry)
WORKDIR /usr/local/src
RUN git clone --depth 1 --branch v1.6 https://github.com/openMVG/openMVG.git
RUN cd openMVG && git submodule update --init --recursive
RUN mkdir openMVG_Build
WORKDIR /usr/local/src/openMVG_Build
RUN cmake -DCMAKE_BUILD_TYPE=RELEASE \
          -DOpenMVG_BUILD_DOC=OFF \
          -DOpenMVG_BUILD_EXAMPLES=OFF \
          -DOpenMVG_BUILD_GUI_SOFTWARES=OFF \
          -DOpenMVG_BUILD_SOFTWARES=OFF \
          -DOpenMVG_USE_OPENMP=OFF \
          -DUSE_OPENMP=OFF \
          -DTARGET_ARCHITECTURE=generic \
          ../openMVG/src
RUN make -j$(nprocs)
RUN make install

# Install VRPN (VR peripheral device network)
WORKDIR /usr/local/src
RUN git clone --depth 1 --branch v07.34 https://github.com/vrpn/vrpn.git
RUN cd vrpn && git submodule update --init --recursive
RUN mkdir vrpn_Build
WORKDIR /usr/local/src/vrpn_Build
RUN cmake -DCMAKE_BUILD_TYPE=RELEASE \
          -DVRPN_USE_GPM_MOUSE=OFF \
          ../vrpn
RUN make -j$(nprocs)
RUN make install

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
# Delete the phony lib
RUN rm src/lib.rs
# Now build the package itself
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
COPY --from=build /usr/local/cargo/bin/hub-server /usr/local/bin/
COPY --from=build /usr/local/cargo/bin/grpc-client /usr/local/bin/
COPY --from=build /usr/local/cargo/bin/vrpn-client /usr/local/bin/
RUN useradd -m posenet
USER posenet
WORKDIR /home/posenet
