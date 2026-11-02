(ns push3-fast-oracle.dump
  "Differential-test data for push3-fast: random programs, run on the clj-push3
  interpreter (the oracle), written as EDN, one case per line, with the complete final state.

  Usage: clojure -M -m push3-fast-oracle.dump <n-programs> <seed> <out.edn> <names.txt>

  <names.txt> is `push3-fast names`: the instructions the Rust build implements. Names the
  oracle does not register are reported and left out.

  A case is {:program (...) :inputs [...] :max n :int [...] :bool [...] :float [...] :char [...]
  :exec [...] :print \"...\" :regs [...] :steps n}. Instructions are keywords."
  (:require
   [benjamin-schwerdtner.clj-push3-play.instructions.input-output :as io]
   [benjamin-schwerdtner.clj-push3-play.instructions.interface :as instr]
   [benjamin-schwerdtner.clj-push3-play.interpreter :as push]
   [clojure.java.io :as io*]
   [clojure.string :as string]))

(defn- registered? [n] (not= :no-instruction (instr/instruction (keyword n))))

(defn- gen-float [^java.util.Random r]
  (let [p (.nextDouble r)]
    (cond
      (< p 0.45) (* (- (.nextDouble r) 0.5) 200.0)
      (< p 0.60) (double (- (.nextInt r 21) 10))
      (< p 0.70) (* (- (.nextDouble r) 0.5) 1e-13)       ;; becomes (float 1e-12) when pushed
      (< p 0.78) (* (if (.nextBoolean r) 1 -1) (+ 1e11 (* (.nextDouble r) 5e12))) ;; clamped
      (< p 0.83) 0.0
      (< p 0.86) Double/NaN
      (< p 0.88) Double/POSITIVE_INFINITY
      (< p 0.90) Double/NEGATIVE_INFINITY
      :else (* (- (.nextDouble r) 0.5) 3.0))))

(defn- gen-char [^java.util.Random r]
  (char (if (< (.nextDouble r) 0.85) (+ 32 (.nextInt r 95)) (.nextInt r 128))))

(defn- gen-int [^java.util.Random r]
  (long (if (< (.nextDouble r) 0.05)
          (* (if (.nextBoolean r) 1 -1) (+ 5e9 (.nextInt r 2000000000)))
          (- (.nextInt r 25) 12))))

(defn- gen-literal [^java.util.Random r types]
  (case (nth types (.nextInt r (count types)))
    :int (gen-int r)
    :bool (.nextBoolean r)
    :float (gen-float r)
    :char (gen-char r)))

(defn- gen-item [^java.util.Random r ops types depth]
  (let [p (.nextDouble r)]
    (cond
      (< p 0.55) (keyword (nth ops (.nextInt r (count ops))))
      (< p 0.88) (gen-literal r types)
      (and (< depth 3) (< p 1.0)) (apply list (repeatedly (inc (.nextInt r 6)) #(gen-item r ops types (inc depth))))
      :else (gen-int r))))

(defn- gen-case [^java.util.Random r ops types]
  {:program (apply list (repeatedly (+ 5 (.nextInt r 36)) #(gen-item r ops types 0)))
   :inputs (vec (repeatedly (.nextInt r 4) #(gen-literal r types)))
   :max (nth [50 300 1000] (.nextInt r 3))})

(def ^:private execute-ns
  "Time spent in the oracle's `execute` alone, without generation, state extraction and printing."
  (atom 0))

(defn- stack [st k] (get-in st [:stacks k]))

(defn- run-oracle [{:keys [program inputs max]}]
  (let [t0 (System/nanoTime)
        st (-> (push/setup-state)
               (assoc :exceptions-are-warnings? true)
               (io/with-inputs inputs)
               (push/execute program {:max-executions max}))
        _ (swap! execute-ns + (- (System/nanoTime) t0))]
    {:int (stack st :push/integer)
     :bool (stack st :push/boolean)
     ;; (double x): the oracle keeps (float 1e-12) as a java.lang.Float; widened it is the exact value
     :float (mapv double (stack st :push/float))
     :char (stack st :push/char)
     :exec (stack st :push/exec)
     :print (io/printed st)
     :regs (mapv #(get-in st [:push/output (keyword (str "c" %))]) (range 8))
     :steps (:push/num-executions st)}))

(defn -main [n seed out names-file]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [names (string/split-lines (slurp names-file))
          ops (filterv registered? names)
          missing (remove registered? names)
          types [:int :bool :float :char]
          r (java.util.Random. (parse-long seed))
          n (parse-long n)
          start (System/nanoTime)
          written (atom 0) failed (atom 0) steps (atom 0)]
      (when (seq missing) (println "not registered in the oracle:" (vec missing)))
      (with-open [w (io*/writer out)]
        (dotimes [_ n]
          (let [c (gen-case r ops types)]
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
