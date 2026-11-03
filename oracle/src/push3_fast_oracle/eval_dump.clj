(ns push3-fast-oracle.eval-dump
  "Data for `push3-fast evaldiff`: a problem file (cases, output kind, error kind) and a programs
  file in which every line carries the oracle's error vector from `run/errors`.

  Usage: ... eval-dump <problem> <n-programs> <n-cases> <seed> <problem-out.edn> <programs-out.edn>"
  (:require
   [benjamin-schwerdtner.problems.propeller.gp :as gp]
   [benjamin-schwerdtner.problems.propeller.problems :as problems]
   [benjamin-schwerdtner.problems.propeller.run :as run]
   [clojure.java.io :as io]
   [random-seed.core :as rs]))

(defn- problem-file [p cases]
  (merge {:output (:output p)
          :error (:error p)
          :max run/default-step-limit
          :cases (mapv #(select-keys % [:inputs :output]) cases)}
         (select-keys p [:num-outputs :input-names])))

(defn dump-problem [problem n-programs n-cases seed problem-out programs-out]
  (binding [*err* (java.io.Writer/nullWriter)]
    (let [p (problems/by-name (keyword problem))
          atoms (problems/atom-generators p)
          cases (:train (problems/train-and-test p (parse-long (str n-cases)) 10 0))
          programs (binding [rs/*rng* (rs/random (parse-long (str seed)))]
                     (vec (repeatedly (parse-long (str n-programs)) #(gp/random-genome atoms 100))))]
      (spit problem-out (pr-str (problem-file p cases)))
      (with-open [w (io/writer programs-out)]
        (doseq [prog programs]
          (.write w (str (pr-str {:program prog :errors (run/errors p prog cases)}) "\n"))))
      (println (format "%s: %d programs, %d cases" problem (count programs) (count cases))))))

(defn -main [problem n-programs n-cases seed problem-out programs-out]
  (dump-problem problem n-programs n-cases seed problem-out programs-out)
  (shutdown-agents))

(defn all-main
  "Every problem into <dir>/<name>.problem.edn and <dir>/<name>.programs.edn, one JVM."
  [n-programs n-cases seed dir]
  (doseq [p problems/all-problems
          :let [n (name (:name p))]]
    (dump-problem n n-programs n-cases seed (str dir "/" n ".problem.edn") (str dir "/" n ".programs.edn")))
  (shutdown-agents))
