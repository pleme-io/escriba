;; escriba-mcp-assist — AI + fleet bridge over MCP (escriba-original).
;; No editor in the category ships typed cross-process MCP import. Each
;; defmcp turns a pleme-io MCP server tool into a first-class command.
(defescribaplugin
  :name          "escriba-mcp-assist"
  :version       "0.1.0"
  :category      "ai"
  :description   "Inline AI edits + chat + fleet MCP-tool bindings (mado / curupira)"
  :blnvim-origin ""
  :ativar-em     ("Event: InsertEnter"))

;; AI inline assistance.
(defkeybind :mode "normal" :key "<leader>ai" :action "ai.inline" :description "AI inline edit")
(defkeybind :mode "normal" :key "<leader>ac" :action "ai.chat"   :description "AI chat")
(defcmd :name "AiInline" :description "AI inline edit at cursor" :action "ai.inline")
(defcmd :name "AiChat"   :description "open the AI chat panel"   :action "ai.chat")

;; Mado clipboard bridge — content-addressed lifecycle.
(defmcp :name "mado.clipboard.get"
        :description "fetch a BLAKE3-addressed clipboard payload from mado"
        :server "mado"
        :tool "clipboard_get"
        :keybind "<leader>mcg"
        :on-result "action:insert-at-cursor")

(defmcp :name "mado.clipboard.put"
        :description "publish the current selection into mado's store"
        :server "mado"
        :tool "clipboard_put"
        :keybind "<leader>mcp")

(defmcp :name "mado.clipboard.list"
        :description "list recent clipboard entries from mado"
        :server "mado"
        :tool "clipboard_list"
        :keybind "<leader>mcl")

(defmcp :name "mado.prompt.list"
        :description "list OSC 133 prompt marks across mado sessions"
        :server "mado"
        :tool "prompt_marks_list"
        :keybind "<leader>mpp")

(defmcp :name "mado.attention.set"
        :description "flip mado's OSC 1337 RequestAttention flag"
        :server "mado"
        :tool "attention_set"
        :keybind "<leader>maf")
