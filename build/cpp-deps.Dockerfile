FROM rust

ARG CPP_DEPS_COMMIT
ENV CPP_DEPS_COMMIT=$CPP_DEPS_COMMIT
RUN echo "CPP_DEPS_COMMIT=$CPP_DEPS_COMMIT"

# Install build dependencies
RUN apt-get update && apt-get install -y libeigen3-dev cmake nmap mlocate

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
