(ns push3-fast-oracle.dump
  "Differential-test data for push3-fast: random programs, run on the clj-push3
  interpreter (the oracle), written as EDN, one case per line, with the complete final state.

  Usage: clojure -M -m push3-fast-oracle.dump <n-programs> <seed> <out.edn> <names.txt>

  <names.txt> is `push3-fast names`: the instructions the Rust build implements. Names the
  oracle does not register are reported and left out.

  A case is {:program (...) :inputs [...] :max n :int [...] :bool [...] :float [...] :char [...]
  :exec [...] :print \"...\" :regs [...] :steps n}. Instructions are keywords."
  (:require
   [benjamin-schwerdtner.clj-push3-play.instructions.impl :as impl]
   [benjamin-schwerdtner.clj-push3-play.instructions.input-output :as io]
   [benjamin-schwerdtner.clj-push3-play.instructions.interface :as instr]
   [benjamin-schwerdtner.clj-push3-play.interpreter :as push]
   [clojure.java.io :as io*]
   [benjamin-schwerdtner.clj-push3-play.stack.pushstate :as stack]
   [clojure.string :as string]))

;; Test-harness patch of the oracle. Upstream :integer_div returns `(bigint ...)`, a clojure BigInt
;; that stays a BigInt after the clamp, so :integer_print shows it as "5N". That is an upstream
;; quirk, not Push semantics, and push3-fast does not reproduce it: here the quotient is a Long.
(impl/register-instruction
 {:sym-name :integer_div
  :in [:push/integer :push/integer]
  :out :state
  :f (fn [state divisor dividend]
       (if (zero? divisor)
         state
         (stack/push state :push/integer (long (/ (Math/floor dividend) (Math/floor divisor))))))})

(defn- registered? [n] (not= :no-instruction (instr/instruction (keyword n))))

(defn- gen-float [^java.util.Random r]
  (let [p (.nextDouble r)]
    (cond
      (< p 0.45) (* (- (.nextDouble r) 0.5) 200.0)
      (< p 0.60) (double (- (.nextInt r 21) 10))
      (< p 0.70) (* (- (.nextDouble r) 0.5) 1e-13)       ;; becomes (float 1e-12) when pushed
      (< p 0.78) (* (if (.nextBoolean r) 1 -1) (+ 1e11 (* (.nextDouble r) 5e12))) ;; clamped
      (< p 0.86) 0.0
      ;; no NaN literals: (= x x) is true for one NaN object (Util.equiv identity), and real
      ;; programs cannot produce NaN, every FLOAT is clamped on push
      (< p 0.88) Double/POSITIVE_INFINITY
      (< p 0.90) Double/NEGATIVE_INFINITY
      :else (* (- (.nextDouble r) 0.5) 3.0))))

(defn- gen-char [^java.util.Random r]
  (char (if (< (.nextDouble r) 0.85) (+ 32 (.nextInt r 95)) (.nextInt r 128))))

(defn- gen-int [^java.util.Random r]
  (long (if (< (.nextDouble r) 0.05)
          (* (if (.nextBoolean r) 1 -1) (+ 5e9 (.nextInt r 2000000000)))
          (- (.nextInt r 25) 12))))

(def ^:private string-alphabet "abcxyzABCXYZ 0123456789.,-+:!_\n\t")

(def ^:private number-alphabet "0123456789012345+-.eE ,\n")

(def ^:private number-samples
  ["0" "-1" "12" "+7" "007" "1.5" "-2.25" "1e3" "1E-2" "3." "42 rest" "  8" "1,2" "5)" "-" "+" "1 2"
   "12abc" "x1" "" "99999999999999999999" "1e400" "0.0000000000001"])

(def ^:private words ["ab" "x" "Cd" "hello" "A" "z9" "-" "0"])

(def ^:private spaces [" " "  " "\t" "\n" " \t" "\u000b" "\f" "\r" "\u001c"])

(defn- pick [^java.util.Random r coll] (nth coll (.nextInt r (count coll))))

(defn- gen-string
  "ASCII only. A mix of: the empty string, plain text, numeric-looking text (for the parsers),
  repeated characters, and words joined by whitespace with leading and trailing whitespace."
  [^java.util.Random r]
  (let [p (.nextDouble r)]
    (cond
      (< p 0.10) ""
      (< p 0.22) (pick r number-samples)
      (< p 0.34) (apply str (repeatedly (inc (.nextInt r 8)) #(.charAt number-alphabet (.nextInt r (count number-alphabet)))))
      (< p 0.42) (apply str (repeat (+ 2 (.nextInt r 4)) (.charAt string-alphabet (.nextInt r (count string-alphabet)))))
      (< p 0.55) (str (when (.nextBoolean r) (pick r spaces))
                      (apply str (interpose (pick r spaces) (repeatedly (inc (.nextInt r 4)) #(pick r words))))
                      (when (.nextBoolean r) (pick r spaces)))
      :else (let [n (inc (.nextInt r 8))]
              (apply str (repeatedly n #(.charAt string-alphabet (.nextInt r (count string-alphabet)))))))))

(defn- gen-ivec
  "Non-empty on purpose: an empty vector literal lands on a [:push/vector :push/clj-object] stack."
  [^java.util.Random r]
  (vec (repeatedly (inc (.nextInt r 5)) #(long (- (.nextInt r 21) 10)))))

(defn- gen-literal [^java.util.Random r types]
  (case (nth types (.nextInt r (count types)))
    :string (gen-string r)
    :ivec (gen-ivec r)
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

(def ^:private modelled-stacks
  #{:push/integer :push/boolean :push/float :push/char :push/string :push/exec :push/print
    [:push/vector :push/integer]})

(defn final-state
  "The comparable final state of `st`, or nil when a stack that push3-fast does not model
  (clj-object, name, code, bignum, other vectors, ...) ended up non-empty."
  [st]
  (when (every? (fn [[k v]] (or (contains? modelled-stacks k) (empty? v))) (:stacks st))
    {:int (stack st :push/integer)
     :bool (stack st :push/boolean)
     ;; (double x): the oracle keeps (float 1e-12) as a java.lang.Float; widened it is the exact value
     :float (mapv double (stack st :push/float))
     :char (stack st :push/char)
     :string (stack st :push/string)
     :vector_integer (stack st [:push/vector :push/integer])
     :exec (stack st :push/exec)
     :print (io/printed st)
     :regs (mapv #(get-in st [:push/output (keyword (str "c" %))]) (range 8))
     :steps (:push/num-executions st)}))

(defn- run-oracle [{:keys [program inputs max]}]
  (let [t0 (System/nanoTime)
        st (-> (push/setup-state)
               (assoc :exceptions-are-warnings? true)
               (io/with-inputs inputs)
               (push/execute program {:max-executions max}))
        _ (swap! execute-ns + (- (System/nanoTime) t0))]
    (final-state st)))

(defn -main [n seed out names-file]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [;; P3F_EXCLUDE=prefix,prefix: leave instructions out (e.g. while another type is unfinished)
          ;; P3F_TYPES=int,bool,float,char,string,ivec: literal types in generated programs and inputs
          exclude (remove string/blank? (string/split (or (System/getenv "P3F_EXCLUDE") "") #","))
          names (remove (fn [n] (some #(string/starts-with? n %) exclude))
                        (string/split-lines (slurp names-file)))
          ops (filterv registered? names)
          missing (remove registered? names)
          types (mapv keyword (string/split (or (System/getenv "P3F_TYPES") "int,bool,float,char,string,ivec") #","))
          r (java.util.Random. (parse-long seed))
          n (parse-long n)
          start (System/nanoTime)
          written (atom 0) failed (atom 0) skipped (atom 0) steps (atom 0)]
      (when (seq missing) (println "not registered in the oracle:" (vec missing)))
      (with-open [w (io*/writer out)]
        (dotimes [_ n]
          (let [c (gen-case r ops types)]
            (try
              (if-let [res (run-oracle c)]
                (do (.write w (str (pr-str (merge c res)) "\n"))
                    (swap! written inc)
                    (swap! steps + (:steps res)))
                (swap! skipped inc))
              (catch Throwable e (swap! failed inc))))))
      (let [dt (/ (- (System/nanoTime) start) 1e9)
            ex (/ @execute-ns 1e9)]
        (println (format "oracle: %d programs written, %d skipped (unmodelled stack non-empty), %d failed, %d steps, %.2f s in total (incl. generation and printing)"
                         @written @skipped @failed @steps dt))
        (println (format "oracle execute only: %.2f s, %.0f runs/s, %.3f M steps/s (one JVM, no warmup removed)"
                         ex (/ @written ex) (/ @steps ex 1e6))))))
  (shutdown-agents))
