(ns push3-fast-oracle.dump
  "Differential-test data for push3-fast: random programs, run on the clj-push3
  interpreter (the oracle), written as EDN, one case per line, with the complete final state.

  Usage: clojure -M -m push3-fast-oracle.dump <n-programs> <seed> <out.edn>

  A case is {:program (...) :inputs [...] :max n :int [...] :bool [...] :exec [...] :steps n}.
  Instructions are keywords. Floats, chars, strings, vectors and names are not part of stage 1."
  (:require
   [benjamin-schwerdtner.clj-push3-play.instructions.input-output :as io]
   [benjamin-schwerdtner.clj-push3-play.instructions.interface :as instr]
   [benjamin-schwerdtner.clj-push3-play.interpreter :as push]
   [clojure.java.io :as io*]))

(def stage1-names
  (let [fams ["stackdepth" "pop" "dup" "flush" "shove" "yank" "yankdup" "rot" "swap"
              "empty" "dup_times" "dup_items" "deep_dup"]]
    (vec (concat
          (for [t ["integer" "boolean" "exec"], f fams] (str t "_" f))
          ["integer_+" "integer_-" "integer_*" "integer_div" "integer_mod" "integer_quot"
           "integer_inc" "integer_dec" "integer_<" "integer_>" "integer_=" "integer_>="
           "integer_<=" "integer_min" "integer_max" "integer_fromboolean"
           "boolean_and" "boolean_or" "boolean_not" "boolean_=" "boolean_xor"
           "boolean_invert_first_then_and" "boolean_invert_second_then_and"
           "boolean_nand" "boolean_nor" "boolean_xnor" "boolean_frominteger"
           "exec_if" "exec_when" "exec_while" "exec_do_while" "exec_k" "exec_s" "exec_y"
           "exec_do*range" "exec_do*count" "exec_do*times" "exec_="]
          (map #(str "in" %) (range 1 9))))))

(defn- registered? [n] (not= :no-instruction (instr/instruction (keyword n))))

(defn- gen-item [^java.util.Random r ops depth]
  (let [p (.nextDouble r)]
    (cond
      (< p 0.55) (keyword (nth ops (.nextInt r (count ops))))
      (< p 0.78) (long (if (< (.nextDouble r) 0.05)
                         (* (if (.nextBoolean r) 1 -1) (+ 5e9 (.nextInt r 2000000000)))
                         (- (.nextInt r 25) 12)))
      (< p 0.88) (.nextBoolean r)
      (and (< depth 3) (< p 1.0)) (apply list (repeatedly (inc (.nextInt r 6)) #(gen-item r ops (inc depth))))
      :else (long (.nextInt r 5)))))

(defn- gen-case [^java.util.Random r ops]
  {:program (apply list (repeatedly (+ 5 (.nextInt r 36)) #(gen-item r ops 0)))
   :inputs (vec (repeatedly (.nextInt r 4)
                            #(if (.nextBoolean r) (long (- (.nextInt r 41) 20)) (.nextBoolean r))))
   :max (nth [50 300 1000] (.nextInt r 3))})

(def ^:private execute-ns
  "Time spent in the oracle's `execute` alone, without generation, state extraction and printing."
  (atom 0))

(defn- run-oracle [{:keys [program inputs max]}]
  (let [t0 (System/nanoTime)
        st (-> (push/setup-state)
               (assoc :exceptions-are-warnings? true)
               (io/with-inputs inputs)
               (push/execute program {:max-executions max}))
        _ (swap! execute-ns + (- (System/nanoTime) t0))]
    {:int (get-in st [:stacks :push/integer])
     :bool (get-in st [:stacks :push/boolean])
     :exec (get-in st [:stacks :push/exec])
     :steps (:push/num-executions st)}))

(defn -main [n seed out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [ops (filterv registered? stage1-names)
          missing (remove registered? stage1-names)
          r (java.util.Random. (parse-long seed))
          n (parse-long n)
          start (System/nanoTime)
          written (atom 0) failed (atom 0) steps (atom 0)]
      (when (seq missing) (println "not registered in the oracle:" (vec missing)))
      (with-open [w (io*/writer out)]
        (dotimes [_ n]
          (let [c (gen-case r ops)]
            (try
              (let [res (run-oracle c)]
                (.write w (str (pr-str (merge c res)) "\n"))
                (swap! written inc)
                (swap! steps + (:steps res)))
              (catch Throwable e (swap! failed inc))))))
      (let [dt (/ (- (System/nanoTime) start) 1e9)
            ex (/ @execute-ns 1e9)]
        (println (format "oracle: %d programs written, %d failed, %d steps, %.2f s in total (incl. generation and printing)"
                         @written @failed @steps dt))
        (println (format "oracle execute only: %.2f s, %.0f runs/s, %.3f M steps/s (one JVM, no warmup removed)"
                         ex (/ @written ex) (/ @steps ex 1e6))))))
  (shutdown-agents))
