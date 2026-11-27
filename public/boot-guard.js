// 前端启动守卫：独立于 Vue 打包产物，在模块脚本之前执行。
//
// 白屏时用户拿不到任何线索，这里把「界面没起来」变成一条可读的错误、
// 一个重新加载入口，并写进后端日志。CSP 只允许 'self' 脚本，因此它必须
// 位于 public/，不能写成 index.html 的内联代码。
(function () {
  var BOOT_TIMEOUT_MS = 30000;
  var mounted = false;
  var failed = false;
  var timer = window.setTimeout(function () {
    show(
      "界面启动超时",
      "前端在 " + BOOT_TIMEOUT_MS / 1000 + " 秒内没有完成挂载。请重新加载；如果反复出现，请附上这段提示和日志。",
    );
  }, BOOT_TIMEOUT_MS);

  function bootShell() {
    var shell = document.getElementById("boot-shell");
    if (shell) return shell;
    // index.html 被替换或占位节点缺失时也要能显示错误，不能退回白屏。
    if (!document.body) return null;
    shell = document.createElement("div");
    shell.id = "boot-shell";
    document.body.insertBefore(shell, document.body.firstChild);
    return shell;
  }

  function clearTimer() {
    if (timer) {
      window.clearTimeout(timer);
      timer = 0;
    }
  }

  function describe(error) {
    if (!error) return "未知错误";
    if (typeof error === "string") return error;
    var message = error.message || String(error);
    var stack = error.stack
      ? String(error.stack).split("\n").slice(0, 8).join("\n")
      : "";
    return stack ? message + "\n\n" + stack : message;
  }

  function button(label, onClick) {
    var node = document.createElement("button");
    node.type = "button";
    node.textContent = label;
    node.addEventListener("click", onClick);
    return node;
  }

  function show(title, detail) {
    // 只有「界面还没挂载」时才接管页面；挂载后的报错留给应用自己处理。
    if (mounted || failed || !bootShell()) return;
    failed = true;
    clearTimer();
    var host = bootShell();
    host.textContent = "";
    var box = document.createElement("div");
    box.className = "boot-failure";
    var heading = document.createElement("strong");
    heading.textContent = title;
    var text = document.createElement("pre");
    text.textContent = detail;
    var actions = document.createElement("div");
    actions.className = "boot-failure-actions";
    var copy = button("复制错误", function () {
      copy.textContent = "复制失败";
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(title + "\n" + detail).then(
          function () {
            copy.textContent = "已复制";
          },
          function () {
            copy.textContent = "复制失败";
          },
        );
      }
    });
    actions.appendChild(button("重新加载", function () {
      window.location.reload();
    }));
    actions.appendChild(copy);
    box.appendChild(heading);
    box.appendChild(text);
    box.appendChild(actions);
    host.appendChild(box);
    try {
      var internals = window.__TAURI_INTERNALS__;
      if (internals && typeof internals.invoke === "function") {
        internals
          .invoke("log_frontend_error", { message: title + "：" + detail })
          .catch(function () {});
      } else if (typeof window.fetch === "function") {
        // 浏览器版由服务端提供页面；同样把启动失败写进服务端日志。
        window
          .fetch("/rpc", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              command: "log_frontend_error",
              args: { message: title + "：" + detail },
            }),
          })
          .catch(function () {});
      }
    } catch (error) {
      // 记录失败不影响屏幕上的错误说明。
    }
  }

  function onError(event) {
    // 资源加载错误没有 error 对象，交给超时兜底，避免把缺图误报成启动失败。
    if (!event || !event.error) return;
    show("界面启动失败", describe(event.error));
  }

  function onRejection(event) {
    show("界面启动失败", describe(event.reason));
  }

  window.__balancehubBoot = {
    mounted: function () {
      mounted = true;
      clearTimer();
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
    },
    fail: function (title, detail) {
      show(title || "界面启动失败", detail || "未知错误");
    },
  };

  window.addEventListener("error", onError);
  window.addEventListener("unhandledrejection", onRejection);
})();
