(ns push3-fast-oracle.bisect
  "Runs one recorded case on the oracle with :max 1..N and writes the final state of each prefix,
  one line per step count, in the same text format as `push3-fast states`.
  Usage: ... bisect <cases.edn> <line-number> <out.txt>"
  (:require
   [benjamin-schwerdtner.clj-push3-play.instructions.input-output :as io]
   [benjamin-schwerdtner.clj-push3-play.interpreter :as push]
   [push3-fast-oracle.dump]
   [clojure.edn :as edn]
   [clojure.java.io :as io*]
   [clojure.string :as string]))

(defn -main [file line out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [c (edn/read-string (nth (string/split-lines (slurp file)) (dec (parse-long line))))]
      (with-open [w (io*/writer out)]
        (doseq [k (range 1 (inc (:max c)))]
          (let [st (-> (push/setup-state) (assoc :exceptions-are-warnings? true) (io/with-inputs (:inputs c))
                       (push/execute (:program c) {:max-executions k}))
                g #(get-in st [:stacks %])]
            (.write w (pr-str k [(g :push/integer) (g :push/boolean) (mapv double (g :push/float)) (g :push/char) (g :push/exec) (io/printed st)]))
            (.write w "\n"))))))
  (shutdown-agents))
