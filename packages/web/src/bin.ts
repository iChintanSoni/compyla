#!/usr/bin/env node
import { createServer } from "./server.js";

const port = Number(process.env.PORT) || 4300;

createServer().listen(port, () => {
  console.log(`compyla-web listening on http://localhost:${port}`);
});
