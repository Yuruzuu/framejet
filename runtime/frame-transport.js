// A shared worker keeps raw pixels out of the page's DevTools network traffic.
onconnect = ({ ports: [port] }) => {
  let socket;
  let pending;

  port.onmessage = ({ data }) => {
    if (data.url) {
      socket = new WebSocket(data.url);
      socket.onopen = () => port.postMessage({ ready: true });
      socket.onerror = socket.onclose = () => port.postMessage({ error: 'Frame stream disconnected' });
      socket.onmessage = ({ data }) => {
        if (data === 'ok') {
          port.postMessage({ buffer: pending }, [pending]);
          pending = null;
        } else port.postMessage({ error: `Frame transfer failed: ${data}` });
      };
    } else if (data.close) {
      socket?.close();
      port.close();
      close();
    } else {
      pending = data.buffer;
      socket.send(pending);
    }
  };
  port.start();
};
