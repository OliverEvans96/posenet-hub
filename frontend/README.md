# PoseNet Hub – Pose Stream Frontend

Real-time 3D pose visualization for the PoseNet Hub. Connects to the hub’s WebSocket pose stream and renders skeletons with Three.js.

## Setup

```bash
yarn install
```

## Development

```bash
yarn dev
```

Opens http://localhost:3000. The app connects to `ws://localhost:9001` by default (hub WebSocket server). Override with `?ws=PORT`, e.g. http://localhost:3000?ws=9002.

## Build

```bash
yarn build
```

Output is in `dist/`.

## Tests

```bash
yarn test
```

Runs unit tests for message parsing and skeleton topology (no UI tests).

## Info bar

- **Connection status** – Connected / Connecting / Disconnected  
- **Group name** – Camera group of the current pose update  
- **Pose count** – Number of poses in the last message  
- **Timestamp** – Time of the last message  
- **Message rate** – Messages per second over the last 2 s
