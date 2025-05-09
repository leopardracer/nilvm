from nada_dsl import *


def nada_main():
    party1 = Party(name="Party1")
    a = SecretInteger(Input(name="a", party=party1))
    b = SecretInteger(Input(name="b", party=party1))

    def add(values: Tuple[SecretInteger, SecretInteger]) -> SecretInteger:
        return values.left + values.right

    left = Array.new(a, a, a)
    right = Array.new(b, b, b)

    additions = left.zip(right).map(add)
    additions = additions.zip(additions).map(add)

    return [Output(additions, "my_output", party1)]
