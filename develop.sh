#!/usr/bin/env bash
nodemon -i target -e 'rs,toml,cpp,hpp,proto,Dockerfile' -x ./build-and-run.sh
