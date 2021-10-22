#!/usr/bin/env bash

IMAGE_NAME="my-posenet"
CONTAINER_NAME="my-posenet-container"

# Build image
docker build --pull -f build/main.Dockerfile . -t $IMAGE_NAME

# Set up trap to kill & remove container when this script exits
trap "docker rm -f $CONTAINER_NAME" EXIT

# Run container
docker run -p 50051:50051 -p 3883:3883 --name $CONTAINER_NAME $IMAGE_NAME
