<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  let espIp = $state("192.168.0.223");
  let count = $state("0");
  let status = $state("");

  async function getCount() {
    status = "Fetching...";
    try {
      const response = await fetch(`http://${espIp}/count`);
      if (!response.ok) throw new Error(`Failed to connect: ${response.statusText}`);
      const text = await response.text();
      count = text;
      status = "Updated";
    } catch (e) {
      console.error(e);
      status = "Error: " + String(e);
    }
  }

  async function increment() {
    status = "Incrementing...";
    try {
      const response = await fetch(`http://${espIp}/add`, { method: 'POST' });
      if (!response.ok) throw new Error(`Failed to connect: ${response.statusText}`);
      // Response is "Added. New count: X"
      await getCount();
      status = "Incremented";
    } catch (e) {
      console.error(e);
      status = "Error: " + String(e);
    }
  }

  async function decrement() {
    status = "Decrementing...";
    try {
      const response = await fetch(`http://${espIp}/sub`, { method: 'POST' });
      if (!response.ok) throw new Error(`Failed to connect: ${response.statusText}`);
      // Response is "Added. New count: X"
      await getCount();
      status = "Decremented";
    } catch (e) {
      console.error(e);
      status = "Error: " + String(e);
    }
  }
</script>

<main class="container">
  <h1>ESP32 Control</h1>
  <h3>Enter your ESP32 IP address below to control the counter.</h3>

  <div class="row">
    <input id="ip-input" placeholder="ESP32 IP Address" bind:value={espIp} />
    <button onclick={getCount}>Connect / Refresh</button>
  </div>

  <div class="counter-section">
    <h2>Count: {count}</h2>
    <button onclick={increment}>+ Increment</button>
    <button onclick={decrement}>- Decrement</button>
  </div>
  
  <p class="status">{status}</p>
</main>

<style>
.counter-section {
  margin-top: 2rem;
  padding: 2rem;
  border: 1px solid #ccc;
  border-radius: 8px;
}

.status {
  margin-top: 1rem;
  font-style: italic;
  color: #888;
}

#ip-input {
  margin-right: 5px;
  width: 200px;
}

:root {
  font-family: Inter, Avenir, Helvetica, Arial, sans-serif;
  font-size: 16px;
  line-height: 24px;
  font-weight: 400;

  color: #0f0f0f;
  background-color: #f6f6f6;

  font-synthesis: none;
  text-rendering: optimizeLegibility;
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  -webkit-text-size-adjust: 100%;
}

.container {
  margin: 0;
  padding-top: 10vh;
  display: flex;
  flex-direction: column;
  justify-content: center;
  text-align: center;
}

.logo {
  height: 6em;
  padding: 1.5em;
  will-change: filter;
  transition: 0.75s;
}

.logo.tauri:hover {
  filter: drop-shadow(0 0 2em #24c8db);
}

.row {
  display: flex;
  justify-content: center;
}

a {
  font-weight: 500;
  color: #646cff;
  text-decoration: inherit;
}

a:hover {
  color: #535bf2;
}

h1 {
  text-align: center;
}

input,
button {
  border-radius: 8px;
  border: 1px solid transparent;
  padding: 0.6em 1.2em;
  font-size: 1em;
  font-weight: 500;
  font-family: inherit;
  color: #0f0f0f;
  background-color: #ffffff;
  transition: border-color 0.25s;
  box-shadow: 0 2px 2px rgba(0, 0, 0, 0.2);
}

button {
  cursor: pointer;
}

button:hover {
  border-color: #396cd8;
}
button:active {
  border-color: #396cd8;
  background-color: #e8e8e8;
}

input,
button {
  outline: none;
}

@media (prefers-color-scheme: dark) {
  :root {
    color: #f6f6f6;
    background-color: #2f2f2f;
  }

  a:hover {
    color: #24c8db;
  }

  input,
  button {
    color: #ffffff;
    background-color: #0f0f0f98;
  }
  button:active {
    background-color: #0f0f0f69;
  }
}

</style>
