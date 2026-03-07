const dot = document.getElementById("dot");
const label = document.getElementById("label");
const hint = document.getElementById("hint");

function update(connected) {
  if (connected) {
    dot.className = "dot on";
    label.textContent = "Connected";
    hint.textContent = "ZeptoBot can control this browser.";
  } else {
    dot.className = "dot off";
    label.textContent = "Disconnected";
    hint.textContent = "Start ZeptoBot to connect.";
  }
}

// Get initial status
chrome.runtime.sendMessage({ type: "get_status" }, (res) => {
  if (res) update(res.connected);
});

// Listen for status changes
chrome.runtime.onMessage.addListener((msg) => {
  if (msg.type === "status") update(msg.connected);
});
