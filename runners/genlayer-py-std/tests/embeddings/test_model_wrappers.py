import numpy as np
import word_piece_tokenizer
from genlayer_embeddings import model_wrappers


class _Tokenizer:
	def tokenize(self, text: str) -> list[int]:
		assert text == 'hello'
		return [1, 2, 3]


def test_sentence_transformer_uses_attention_mask(monkeypatch):
	received = {}

	def model(**kwargs):
		received.update(kwargs)
		return {'embedding': np.array([[1.0, 2.0]])}

	monkeypatch.setattr(word_piece_tokenizer, 'WordPieceTokenizer', _Tokenizer)
	monkeypatch.setattr(model_wrappers, 'get_model', lambda *_args, **_kwargs: model)
	monkeypatch.setattr(
		model_wrappers,
		'_ALL_MODELS',
		{'same-key': {}},
	)
	model_wrappers._cache_sentence_transformer_by_model.clear()

	result = model_wrappers.SentenceTransformer('same-key')('hello')

	assert result.tolist() == [1.0, 2.0]
	assert received['attention_mask'].tolist() == [[1, 1, 1]]
	assert received['token_type_ids'].tolist() == [[0, 0, 0]]


def test_model_and_path_caches_do_not_collide(monkeypatch):
	monkeypatch.setattr(word_piece_tokenizer, 'WordPieceTokenizer', _Tokenizer)
	monkeypatch.setattr(
		model_wrappers,
		'get_model',
		lambda *_args, **_kwargs: lambda **_inputs: {'embedding': np.array([[1.0]])},
	)
	monkeypatch.setattr(model_wrappers, '_ALL_MODELS', {'same-key': {}})
	model_wrappers._cache_sentence_transformer_by_model.clear()
	model_wrappers._cache_sentence_transformer_by_path.clear()

	model_wrapper = model_wrappers.SentenceTransformer('same-key')
	monkeypatch.setattr(
		model_wrappers.Path,
		'read_text',
		lambda _self: "def main(**_inputs):\n\treturn {'embedding': None}\n",
	)
	path_wrapper = model_wrappers.SentenceTransformerFromPath('same-key')

	assert model_wrapper is not path_wrapper
