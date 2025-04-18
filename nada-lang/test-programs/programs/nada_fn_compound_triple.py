from nada_dsl import *


def nada_main():
    """Tests compound Nada functions"""
    party1 = Party(name="Party1")
    my_int1 = SecretInteger(Input(name="my_int1", party=party1))
    my_int2 = SecretInteger(Input(name="my_int2", party=party1))

    def add(a: SecretInteger, b: SecretInteger) -> SecretInteger:
        return a + b

    def add_times(a: SecretInteger, b: SecretInteger) -> SecretInteger:
        return a * add(a, b)

    def add_add_times(a: SecretInteger, b: SecretInteger) -> SecretInteger:
        return b + add_times(a, b)

    new_int = add_add_times(my_int1, my_int2)
    return [Output(new_int, "my_output", party1)]
